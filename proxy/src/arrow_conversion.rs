/// Shared Arrow to PostgreSQL wire protocol conversion utilities
/// Optimized for batch processing with pre-downcasting columns
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use datafusion::arrow::array::*;
use datafusion::arrow::datatypes::{DataType as ArrowDataType, IntervalUnit, TimeUnit};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::arrow::util::display::{ArrayFormatter, FormatOptions};
use pg_interval::Interval as PgInterval;
use pgwire::api::Type;
use pgwire::api::results::{DataRowEncoder, FieldFormat, FieldInfo};
use pgwire::error::{PgWireError, PgWireResult};
use rust_decimal::Decimal;
use std::sync::Arc;

fn downcast_err(type_name: &str) -> PgWireError {
    PgWireError::ApiError(format!("Failed to downcast column to {type_name}").into())
}

/// Convert Arrow DataType to pgwire Type
pub fn arrow_type_to_pgwire(arrow_type: &ArrowDataType) -> Type {
    match arrow_type {
        ArrowDataType::Int8 | ArrowDataType::Int16 => Type::INT2,
        ArrowDataType::Int32 => Type::INT4,
        ArrowDataType::Int64 => Type::INT8,
        ArrowDataType::UInt8 => Type::INT2,
        ArrowDataType::UInt16 => Type::INT4, // u16 max (65535) exceeds i16 max (32767)
        ArrowDataType::UInt32 => Type::INT8, // u32 max (4294967295) exceeds i32 max
        ArrowDataType::UInt64 => Type::INT8,
        ArrowDataType::Float32 => Type::FLOAT4,
        ArrowDataType::Float64 => Type::FLOAT8,
        ArrowDataType::Boolean => Type::BOOL,
        ArrowDataType::Utf8 | ArrowDataType::LargeUtf8 => Type::VARCHAR,
        ArrowDataType::Date32 | ArrowDataType::Date64 => Type::DATE,
        ArrowDataType::Timestamp(_, None) => Type::TIMESTAMP,
        ArrowDataType::Timestamp(_, Some(_)) => Type::TIMESTAMPTZ,
        ArrowDataType::Binary | ArrowDataType::LargeBinary | ArrowDataType::FixedSizeBinary(_) => {
            Type::BYTEA
        }
        ArrowDataType::Decimal128(_, _) | ArrowDataType::Decimal256(_, _) => Type::NUMERIC,
        ArrowDataType::Interval(_) => Type::INTERVAL,
        ArrowDataType::List(f) | ArrowDataType::LargeList(f) => {
            match arrow_type_to_pgwire(f.data_type()) {
                Type::INT2 => Type::INT2_ARRAY,
                Type::INT4 => Type::INT4_ARRAY,
                Type::INT8 => Type::INT8_ARRAY,
                Type::FLOAT4 => Type::FLOAT4_ARRAY,
                Type::FLOAT8 => Type::FLOAT8_ARRAY,
                Type::BOOL => Type::BOOL_ARRAY,
                Type::VARCHAR => Type::VARCHAR_ARRAY,
                Type::BYTEA => Type::BYTEA_ARRAY,
                _ => Type::VARCHAR,
            }
        }
        _ => Type::VARCHAR,
    }
}

/// Build field info from Arrow schema
/// Always uses Text format to avoid binary encoding issues with Extended Query Protocol
pub fn build_field_info(schema: &Arc<datafusion::arrow::datatypes::Schema>) -> Arc<Vec<FieldInfo>> {
    let fields: Vec<FieldInfo> = schema
        .fields()
        .iter()
        .map(|field| {
            FieldInfo::new(
                field.name().clone(),
                None,
                None,
                arrow_type_to_pgwire(field.data_type()),
                FieldFormat::Text,
            )
        })
        .collect();
    Arc::new(fields)
}

/// Trait for encoding values from a specific column type
trait ColumnEncoder: Send + Sync {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()>;
}

// ── Primitive integer encoders ────────────────────────────────────────────────

struct Int8Encoder {
    array: Int8Array,
}
impl ColumnEncoder for Int8Encoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<i16>)
        } else {
            encoder.encode_field(&Some(self.array.value(row_idx) as i16))
        }
    }
}

struct Int16Encoder {
    array: Int16Array,
}
impl ColumnEncoder for Int16Encoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<i16>)
        } else {
            encoder.encode_field(&Some(self.array.value(row_idx)))
        }
    }
}

struct Int32Encoder {
    array: Int32Array,
}
impl ColumnEncoder for Int32Encoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<i32>)
        } else {
            encoder.encode_field(&Some(self.array.value(row_idx)))
        }
    }
}

struct Int64Encoder {
    array: Int64Array,
}
impl ColumnEncoder for Int64Encoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<i64>)
        } else {
            encoder.encode_field(&Some(self.array.value(row_idx)))
        }
    }
}

// ── Unsigned integer encoders (widened to signed) ─────────────────────────────

struct UInt8Encoder {
    array: UInt8Array,
}
impl ColumnEncoder for UInt8Encoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<i16>)
        } else {
            encoder.encode_field(&Some(self.array.value(row_idx) as i16))
        }
    }
}

struct UInt16Encoder {
    array: UInt16Array,
}
impl ColumnEncoder for UInt16Encoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<i32>)
        } else {
            encoder.encode_field(&Some(self.array.value(row_idx) as i32))
        }
    }
}

struct UInt32Encoder {
    array: UInt32Array,
}
impl ColumnEncoder for UInt32Encoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<i64>)
        } else {
            encoder.encode_field(&Some(self.array.value(row_idx) as i64))
        }
    }
}

struct UInt64Encoder {
    array: UInt64Array,
}
impl ColumnEncoder for UInt64Encoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<i64>)
        } else {
            let v = self.array.value(row_idx);
            let signed = i64::try_from(v).map_err(|_| {
                PgWireError::ApiError(format!("UInt64 value {v} overflows INT8").into())
            })?;
            encoder.encode_field(&Some(signed))
        }
    }
}

// ── Float encoders ────────────────────────────────────────────────────────────

struct Float32Encoder {
    array: Float32Array,
}
impl ColumnEncoder for Float32Encoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<f32>)
        } else {
            encoder.encode_field(&Some(self.array.value(row_idx)))
        }
    }
}

struct Float64Encoder {
    array: Float64Array,
}
impl ColumnEncoder for Float64Encoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<f64>)
        } else {
            encoder.encode_field(&Some(self.array.value(row_idx)))
        }
    }
}

// ── Boolean encoder ───────────────────────────────────────────────────────────

struct BooleanEncoder {
    array: BooleanArray,
}
impl ColumnEncoder for BooleanEncoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<bool>)
        } else {
            encoder.encode_field(&Some(self.array.value(row_idx)))
        }
    }
}

// ── String encoders ───────────────────────────────────────────────────────────

struct Utf8Encoder {
    array: StringArray,
}
impl ColumnEncoder for Utf8Encoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<&str>)
        } else {
            encoder.encode_field(&Some(self.array.value(row_idx)))
        }
    }
}

struct LargeUtf8Encoder {
    array: LargeStringArray,
}
impl ColumnEncoder for LargeUtf8Encoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<&str>)
        } else {
            encoder.encode_field(&Some(self.array.value(row_idx)))
        }
    }
}

// ── Binary encoders ───────────────────────────────────────────────────────────

struct BinaryEncoder {
    array: BinaryArray,
}
impl ColumnEncoder for BinaryEncoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<&[u8]>)
        } else {
            encoder.encode_field(&Some(self.array.value(row_idx)))
        }
    }
}

struct LargeBinaryEncoder {
    array: LargeBinaryArray,
}
impl ColumnEncoder for LargeBinaryEncoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<&[u8]>)
        } else {
            encoder.encode_field(&Some(self.array.value(row_idx)))
        }
    }
}

// ── Date encoder ──────────────────────────────────────────────────────────────

/// Arrow Date32 stores days since Unix epoch (1970-01-01).
struct Date32Encoder {
    array: Date32Array,
}
impl ColumnEncoder for Date32Encoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<NaiveDate>)
        } else {
            let days = self.array.value(row_idx);
            let date = NaiveDate::from_ymd_opt(1970, 1, 1)
                .and_then(|epoch| {
                    epoch
                        .checked_add_days(chrono::Days::new(days.unsigned_abs() as u64))
                        .filter(|_| days >= 0)
                        .or_else(|| {
                            NaiveDate::from_ymd_opt(1970, 1, 1).and_then(|epoch| {
                                epoch.checked_sub_days(chrono::Days::new((-days) as u64))
                            })
                        })
                })
                .ok_or_else(|| {
                    PgWireError::ApiError(format!("Invalid Date32 value: {days}").into())
                })?;
            encoder.encode_field(&Some(date))
        }
    }
}

// ── Timestamp encoders ────────────────────────────────────────────────────────

/// Helper: convert microseconds-since-epoch to NaiveDateTime.
fn micros_to_naive(micros: i64) -> Option<NaiveDateTime> {
    DateTime::from_timestamp_micros(micros).map(|dt| dt.naive_utc())
}

fn millis_to_naive(ms: i64) -> Option<NaiveDateTime> {
    DateTime::from_timestamp_millis(ms).map(|dt| dt.naive_utc())
}

fn seconds_to_naive(s: i64) -> Option<NaiveDateTime> {
    DateTime::from_timestamp(s, 0).map(|dt| dt.naive_utc())
}

fn nanos_to_naive(ns: i64) -> Option<NaiveDateTime> {
    let secs = ns / 1_000_000_000;
    let nsecs = (ns % 1_000_000_000).unsigned_abs() as u32;
    DateTime::from_timestamp(secs, nsecs).map(|dt| dt.naive_utc())
}

struct TimestampEncoder {
    array: TimestampMicrosecondArray,
    has_tz: bool,
}
impl ColumnEncoder for TimestampEncoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            if self.has_tz {
                encoder.encode_field(&None::<DateTime<Utc>>)
            } else {
                encoder.encode_field(&None::<NaiveDateTime>)
            }
        } else {
            let micros = self.array.value(row_idx);
            let ndt = micros_to_naive(micros).ok_or_else(|| {
                PgWireError::ApiError(format!("Invalid timestamp micros: {micros}").into())
            })?;
            if self.has_tz {
                encoder.encode_field(&Some(DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc)))
            } else {
                encoder.encode_field(&Some(ndt))
            }
        }
    }
}

struct TimestampMillisEncoder {
    array: TimestampMillisecondArray,
    has_tz: bool,
}
impl ColumnEncoder for TimestampMillisEncoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            if self.has_tz {
                encoder.encode_field(&None::<DateTime<Utc>>)
            } else {
                encoder.encode_field(&None::<NaiveDateTime>)
            }
        } else {
            let ms = self.array.value(row_idx);
            let ndt = millis_to_naive(ms).ok_or_else(|| {
                PgWireError::ApiError(format!("Invalid timestamp millis: {ms}").into())
            })?;
            if self.has_tz {
                encoder.encode_field(&Some(DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc)))
            } else {
                encoder.encode_field(&Some(ndt))
            }
        }
    }
}

struct TimestampSecondsEncoder {
    array: TimestampSecondArray,
    has_tz: bool,
}
impl ColumnEncoder for TimestampSecondsEncoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            if self.has_tz {
                encoder.encode_field(&None::<DateTime<Utc>>)
            } else {
                encoder.encode_field(&None::<NaiveDateTime>)
            }
        } else {
            let s = self.array.value(row_idx);
            let ndt = seconds_to_naive(s).ok_or_else(|| {
                PgWireError::ApiError(format!("Invalid timestamp seconds: {s}").into())
            })?;
            if self.has_tz {
                encoder.encode_field(&Some(DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc)))
            } else {
                encoder.encode_field(&Some(ndt))
            }
        }
    }
}

struct TimestampNanosEncoder {
    array: TimestampNanosecondArray,
    has_tz: bool,
}
impl ColumnEncoder for TimestampNanosEncoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            if self.has_tz {
                encoder.encode_field(&None::<DateTime<Utc>>)
            } else {
                encoder.encode_field(&None::<NaiveDateTime>)
            }
        } else {
            let ns = self.array.value(row_idx);
            let ndt = nanos_to_naive(ns).ok_or_else(|| {
                PgWireError::ApiError(format!("Invalid timestamp nanos: {ns}").into())
            })?;
            if self.has_tz {
                encoder.encode_field(&Some(DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc)))
            } else {
                encoder.encode_field(&Some(ndt))
            }
        }
    }
}

// ── Decimal encoder ───────────────────────────────────────────────────────────

struct Decimal128Encoder {
    array: Decimal128Array,
    scale: u32,
}
impl ColumnEncoder for Decimal128Encoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<Decimal>)
        } else {
            let raw = self.array.value(row_idx);
            let dec = Decimal::try_from_i128_with_scale(raw, self.scale)
                .map_err(|e| PgWireError::ApiError(Box::new(e)))?;
            encoder.encode_field(&Some(dec))
        }
    }
}

// ── Interval encoders ─────────────────────────────────────────────────────────

struct IntervalYearMonthEncoder {
    array: IntervalYearMonthArray,
}
impl ColumnEncoder for IntervalYearMonthEncoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<PgInterval>)
        } else {
            let months = self.array.value(row_idx);
            encoder.encode_field(&Some(PgInterval::new(months, 0, 0)))
        }
    }
}

struct IntervalDayTimeEncoder {
    array: IntervalDayTimeArray,
}
impl ColumnEncoder for IntervalDayTimeEncoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<PgInterval>)
        } else {
            let v = self.array.value(row_idx);
            // Arrow IntervalDayTime: upper 32 bits = days, lower 32 bits = milliseconds
            let days = v.days;
            let micros = v.milliseconds as i64 * 1_000;
            encoder.encode_field(&Some(PgInterval::new(0, days, micros)))
        }
    }
}

struct IntervalMonthDayNanoEncoder {
    array: IntervalMonthDayNanoArray,
}
impl ColumnEncoder for IntervalMonthDayNanoEncoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        if self.array.is_null(row_idx) {
            encoder.encode_field(&None::<PgInterval>)
        } else {
            let v = self.array.value(row_idx);
            let micros = v.nanoseconds / 1_000;
            encoder.encode_field(&Some(PgInterval::new(v.months, v.days, micros)))
        }
    }
}

// ── Fallback string encoder ───────────────────────────────────────────────────

struct GenericEncoder {
    values: Vec<Option<String>>,
}
impl ColumnEncoder for GenericEncoder {
    fn encode_value(&self, row_idx: usize, encoder: &mut DataRowEncoder) -> PgWireResult<()> {
        encoder.encode_field(&self.values[row_idx])
    }
}

// ── Encoder factory ───────────────────────────────────────────────────────────

/// Create a column encoder for a given Arrow array.
/// Downcasts the column ONCE per batch instead of once per cell.
fn create_column_encoder(
    column: &Arc<dyn datafusion::arrow::array::Array>,
) -> PgWireResult<Box<dyn ColumnEncoder>> {
    match column.data_type() {
        ArrowDataType::Int8 => {
            let array = column
                .as_any()
                .downcast_ref::<Int8Array>()
                .ok_or_else(|| downcast_err("Int8Array"))?
                .clone();
            Ok(Box::new(Int8Encoder { array }))
        }
        ArrowDataType::Int16 => {
            let array = column
                .as_any()
                .downcast_ref::<Int16Array>()
                .ok_or_else(|| downcast_err("Int16Array"))?
                .clone();
            Ok(Box::new(Int16Encoder { array }))
        }
        ArrowDataType::Int32 => {
            let array = column
                .as_any()
                .downcast_ref::<Int32Array>()
                .ok_or_else(|| downcast_err("Int32Array"))?
                .clone();
            Ok(Box::new(Int32Encoder { array }))
        }
        ArrowDataType::Int64 => {
            let array = column
                .as_any()
                .downcast_ref::<Int64Array>()
                .ok_or_else(|| downcast_err("Int64Array"))?
                .clone();
            Ok(Box::new(Int64Encoder { array }))
        }
        ArrowDataType::UInt8 => {
            let array = column
                .as_any()
                .downcast_ref::<UInt8Array>()
                .ok_or_else(|| downcast_err("UInt8Array"))?
                .clone();
            Ok(Box::new(UInt8Encoder { array }))
        }
        ArrowDataType::UInt16 => {
            let array = column
                .as_any()
                .downcast_ref::<UInt16Array>()
                .ok_or_else(|| downcast_err("UInt16Array"))?
                .clone();
            Ok(Box::new(UInt16Encoder { array }))
        }
        ArrowDataType::UInt32 => {
            let array = column
                .as_any()
                .downcast_ref::<UInt32Array>()
                .ok_or_else(|| downcast_err("UInt32Array"))?
                .clone();
            Ok(Box::new(UInt32Encoder { array }))
        }
        ArrowDataType::UInt64 => {
            let array = column
                .as_any()
                .downcast_ref::<UInt64Array>()
                .ok_or_else(|| downcast_err("UInt64Array"))?
                .clone();
            Ok(Box::new(UInt64Encoder { array }))
        }
        ArrowDataType::Float32 => {
            let array = column
                .as_any()
                .downcast_ref::<Float32Array>()
                .ok_or_else(|| downcast_err("Float32Array"))?
                .clone();
            Ok(Box::new(Float32Encoder { array }))
        }
        ArrowDataType::Float64 => {
            let array = column
                .as_any()
                .downcast_ref::<Float64Array>()
                .ok_or_else(|| downcast_err("Float64Array"))?
                .clone();
            Ok(Box::new(Float64Encoder { array }))
        }
        ArrowDataType::Boolean => {
            let array = column
                .as_any()
                .downcast_ref::<BooleanArray>()
                .ok_or_else(|| downcast_err("BooleanArray"))?
                .clone();
            Ok(Box::new(BooleanEncoder { array }))
        }
        ArrowDataType::Utf8 => {
            let array = column
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| downcast_err("StringArray"))?
                .clone();
            Ok(Box::new(Utf8Encoder { array }))
        }
        ArrowDataType::LargeUtf8 => {
            let array = column
                .as_any()
                .downcast_ref::<LargeStringArray>()
                .ok_or_else(|| downcast_err("LargeStringArray"))?
                .clone();
            Ok(Box::new(LargeUtf8Encoder { array }))
        }
        ArrowDataType::Binary => {
            let array = column
                .as_any()
                .downcast_ref::<BinaryArray>()
                .ok_or_else(|| downcast_err("BinaryArray"))?
                .clone();
            Ok(Box::new(BinaryEncoder { array }))
        }
        ArrowDataType::LargeBinary => {
            let array = column
                .as_any()
                .downcast_ref::<LargeBinaryArray>()
                .ok_or_else(|| downcast_err("LargeBinaryArray"))?
                .clone();
            Ok(Box::new(LargeBinaryEncoder { array }))
        }
        ArrowDataType::Date32 => {
            let array = column
                .as_any()
                .downcast_ref::<Date32Array>()
                .ok_or_else(|| downcast_err("Date32Array"))?
                .clone();
            Ok(Box::new(Date32Encoder { array }))
        }
        ArrowDataType::Timestamp(TimeUnit::Microsecond, tz) => {
            let array = column
                .as_any()
                .downcast_ref::<TimestampMicrosecondArray>()
                .ok_or_else(|| downcast_err("TimestampMicrosecondArray"))?
                .clone();
            Ok(Box::new(TimestampEncoder {
                array,
                has_tz: tz.is_some(),
            }))
        }
        ArrowDataType::Timestamp(TimeUnit::Millisecond, tz) => {
            let array = column
                .as_any()
                .downcast_ref::<TimestampMillisecondArray>()
                .ok_or_else(|| downcast_err("TimestampMillisecondArray"))?
                .clone();
            Ok(Box::new(TimestampMillisEncoder {
                array,
                has_tz: tz.is_some(),
            }))
        }
        ArrowDataType::Timestamp(TimeUnit::Second, tz) => {
            let array = column
                .as_any()
                .downcast_ref::<TimestampSecondArray>()
                .ok_or_else(|| downcast_err("TimestampSecondArray"))?
                .clone();
            Ok(Box::new(TimestampSecondsEncoder {
                array,
                has_tz: tz.is_some(),
            }))
        }
        ArrowDataType::Timestamp(TimeUnit::Nanosecond, tz) => {
            let array = column
                .as_any()
                .downcast_ref::<TimestampNanosecondArray>()
                .ok_or_else(|| downcast_err("TimestampNanosecondArray"))?
                .clone();
            Ok(Box::new(TimestampNanosEncoder {
                array,
                has_tz: tz.is_some(),
            }))
        }
        ArrowDataType::Decimal128(_, scale) => {
            let scale = *scale as u32;
            let array = column
                .as_any()
                .downcast_ref::<Decimal128Array>()
                .ok_or_else(|| downcast_err("Decimal128Array"))?
                .clone();
            Ok(Box::new(Decimal128Encoder { array, scale }))
        }
        ArrowDataType::Interval(IntervalUnit::YearMonth) => {
            let array = column
                .as_any()
                .downcast_ref::<IntervalYearMonthArray>()
                .ok_or_else(|| downcast_err("IntervalYearMonthArray"))?
                .clone();
            Ok(Box::new(IntervalYearMonthEncoder { array }))
        }
        ArrowDataType::Interval(IntervalUnit::DayTime) => {
            let array = column
                .as_any()
                .downcast_ref::<IntervalDayTimeArray>()
                .ok_or_else(|| downcast_err("IntervalDayTimeArray"))?
                .clone();
            Ok(Box::new(IntervalDayTimeEncoder { array }))
        }
        ArrowDataType::Interval(IntervalUnit::MonthDayNano) => {
            let array = column
                .as_any()
                .downcast_ref::<IntervalMonthDayNanoArray>()
                .ok_or_else(|| downcast_err("IntervalMonthDayNanoArray"))?
                .clone();
            Ok(Box::new(IntervalMonthDayNanoEncoder { array }))
        }
        _ => {
            // For unsupported types, pre-convert all values to strings once per batch
            let options = FormatOptions::default();
            let formatter = ArrayFormatter::try_new(column.as_ref(), &options)
                .map_err(|e| PgWireError::ApiError(Box::new(e)))?;
            let mut values = Vec::with_capacity(column.len());
            for i in 0..column.len() {
                if column.is_null(i) {
                    values.push(None);
                } else {
                    values.push(Some(formatter.value(i).to_string()));
                }
            }
            Ok(Box::new(GenericEncoder { values }))
        }
    }
}

// ── Batch encoder ─────────────────────────────────────────────────────────────

/// Encode a RecordBatch to pgwire DataRows.
/// Columns are downcast ONCE per batch (not once per cell) for efficiency.
pub fn encode_batch_optimized(
    batch: RecordBatch,
    fields: Arc<Vec<FieldInfo>>,
) -> PgWireResult<Vec<pgwire::messages::data::DataRow>> {
    #[cfg(debug_assertions)]
    let start = std::time::Instant::now();

    let column_encoders: Vec<Box<dyn ColumnEncoder>> = batch
        .columns()
        .iter()
        .map(|col| create_column_encoder(col))
        .collect::<PgWireResult<Vec<_>>>()?;

    #[cfg(debug_assertions)]
    let downcast_time = start.elapsed();
    #[cfg(debug_assertions)]
    let encode_start = std::time::Instant::now();

    let mut rows = Vec::with_capacity(batch.num_rows());
    for row_idx in 0..batch.num_rows() {
        let mut encoder = DataRowEncoder::new(fields.clone());
        for col_encoder in &column_encoders {
            col_encoder.encode_value(row_idx, &mut encoder)?;
        }
        rows.push(encoder.take_row());
    }

    #[cfg(debug_assertions)]
    tracing::trace!(
        rows = batch.num_rows(),
        downcast = ?downcast_time,
        encode = ?encode_start.elapsed(),
        total = ?start.elapsed(),
        "Batch encoding"
    );

    Ok(rows)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{
        BinaryArray, BooleanArray, Date32Array, Decimal128Array, Float64Array, Int32Array,
        IntervalDayTimeArray, IntervalMonthDayNanoArray, IntervalYearMonthArray, StringArray,
        TimestampMicrosecondArray, UInt16Array, UInt32Array, UInt64Array,
    };
    use arrow::datatypes::{
        DataType as ArrowDataType, Field, IntervalDayTime, IntervalMonthDayNano, IntervalUnit,
        Schema, TimeUnit,
    };
    use arrow::record_batch::RecordBatch;
    use std::sync::Arc;

    // ── Type mapping tests ────────────────────────────────────────────────────

    #[test]
    fn test_type_mapping_integers() {
        assert_eq!(arrow_type_to_pgwire(&ArrowDataType::Int8), Type::INT2);
        assert_eq!(arrow_type_to_pgwire(&ArrowDataType::Int16), Type::INT2);
        assert_eq!(arrow_type_to_pgwire(&ArrowDataType::Int32), Type::INT4);
        assert_eq!(arrow_type_to_pgwire(&ArrowDataType::Int64), Type::INT8);
        assert_eq!(arrow_type_to_pgwire(&ArrowDataType::UInt8), Type::INT2);
        assert_eq!(arrow_type_to_pgwire(&ArrowDataType::UInt16), Type::INT4);
        assert_eq!(arrow_type_to_pgwire(&ArrowDataType::UInt32), Type::INT8);
        assert_eq!(arrow_type_to_pgwire(&ArrowDataType::UInt64), Type::INT8);
    }

    #[test]
    fn test_type_mapping_temporal() {
        assert_eq!(arrow_type_to_pgwire(&ArrowDataType::Date32), Type::DATE);
        assert_eq!(arrow_type_to_pgwire(&ArrowDataType::Date64), Type::DATE);
        assert_eq!(
            arrow_type_to_pgwire(&ArrowDataType::Timestamp(TimeUnit::Microsecond, None)),
            Type::TIMESTAMP
        );
        assert_eq!(
            arrow_type_to_pgwire(&ArrowDataType::Timestamp(
                TimeUnit::Microsecond,
                Some("UTC".into())
            )),
            Type::TIMESTAMPTZ
        );
        assert_eq!(
            arrow_type_to_pgwire(&ArrowDataType::Interval(IntervalUnit::YearMonth)),
            Type::INTERVAL
        );
    }

    #[test]
    fn test_type_mapping_numeric_and_binary() {
        assert_eq!(
            arrow_type_to_pgwire(&ArrowDataType::Decimal128(10, 2)),
            Type::NUMERIC
        );
        assert_eq!(arrow_type_to_pgwire(&ArrowDataType::Binary), Type::BYTEA);
        assert_eq!(
            arrow_type_to_pgwire(&ArrowDataType::LargeBinary),
            Type::BYTEA
        );
    }

    #[test]
    fn test_type_mapping_unknown_defaults_to_varchar() {
        assert_eq!(
            arrow_type_to_pgwire(&ArrowDataType::Duration(TimeUnit::Second)),
            Type::VARCHAR
        );
    }

    // ── Encoding tests ────────────────────────────────────────────────────────

    fn encode(
        schema: Arc<Schema>,
        columns: Vec<Arc<dyn Array>>,
    ) -> Vec<pgwire::messages::data::DataRow> {
        let batch = RecordBatch::try_new(schema.clone(), columns).expect("RecordBatch::try_new");
        let fields = build_field_info(&schema);
        encode_batch_optimized(batch, fields).expect("encode_batch_optimized")
    }

    #[test]
    fn test_encode_basic_types() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", ArrowDataType::Int32, false),
            Field::new("name", ArrowDataType::Utf8, false),
            Field::new("score", ArrowDataType::Float64, false),
            Field::new("active", ArrowDataType::Boolean, false),
        ]));
        let rows = encode(
            schema,
            vec![
                Arc::new(Int32Array::from(vec![1, 2, 3])),
                Arc::new(StringArray::from(vec!["a", "b", "c"])),
                Arc::new(Float64Array::from(vec![1.0, 2.0, 3.0])),
                Arc::new(BooleanArray::from(vec![true, false, true])),
            ],
        );
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn test_encode_with_nulls() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", ArrowDataType::Int32, true),
            Field::new("name", ArrowDataType::Utf8, true),
        ]));
        let rows = encode(
            schema,
            vec![
                Arc::new(Int32Array::from(vec![Some(1), None, Some(3)])),
                Arc::new(StringArray::from(vec![Some("x"), None, Some("z")])),
            ],
        );
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn test_encode_empty_batch() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "id",
            ArrowDataType::Int32,
            false,
        )]));
        let rows = encode(schema, vec![Arc::new(Int32Array::from(Vec::<i32>::new()))]);
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn test_encode_date32() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "d",
            ArrowDataType::Date32,
            true,
        )]));
        // 0 = 1970-01-01, 1 = 1970-01-02, negative = before epoch
        let rows = encode(
            schema,
            vec![Arc::new(Date32Array::from(vec![
                Some(0),
                Some(1),
                Some(18628),
                None,
            ]))],
        );
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn test_encode_timestamp_micros_no_tz() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "ts",
            ArrowDataType::Timestamp(TimeUnit::Microsecond, None),
            true,
        )]));
        let rows = encode(
            schema,
            vec![Arc::new(TimestampMicrosecondArray::from(vec![
                Some(0),
                Some(1_000_000),
                None,
            ]))],
        );
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn test_encode_timestamp_micros_with_tz() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "ts",
            ArrowDataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            true,
        )]));
        // Array must carry the same timezone metadata as the schema field.
        let array = TimestampMicrosecondArray::from(vec![Some(0), Some(1_000_000), None])
            .with_timezone("UTC");
        let rows = encode(schema, vec![Arc::new(array)]);
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn test_encode_decimal128() {
        // Decimal128(10, 2): value 12345 means 123.45
        let schema = Arc::new(Schema::new(vec![Field::new(
            "price",
            ArrowDataType::Decimal128(10, 2),
            true,
        )]));
        let array = Decimal128Array::from(vec![Some(12345i128), Some(-99), None])
            .with_precision_and_scale(10, 2)
            .unwrap();
        let rows = encode(schema, vec![Arc::new(array)]);
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn test_encode_binary() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "data",
            ArrowDataType::Binary,
            true,
        )]));
        let rows = encode(
            schema,
            vec![Arc::new(BinaryArray::from_opt_vec(vec![
                Some(b"hello".as_ref()),
                None,
                Some(b"\xDE\xAD\xBE\xEF".as_ref()),
            ]))],
        );
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn test_encode_uint64_overflow_returns_error() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "big",
            ArrowDataType::UInt64,
            false,
        )]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(UInt64Array::from(vec![u64::MAX]))],
        )
        .expect("RecordBatch::try_new");
        let fields = build_field_info(&schema);
        let result = encode_batch_optimized(batch, fields);
        assert!(result.is_err(), "Expected overflow error for u64::MAX");
    }

    #[test]
    fn test_encode_interval_year_month() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "iv",
            ArrowDataType::Interval(IntervalUnit::YearMonth),
            true,
        )]));
        let rows = encode(
            schema,
            vec![Arc::new(IntervalYearMonthArray::from(vec![
                Some(12),
                None,
                Some(-1),
            ]))],
        );
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn test_encode_interval_day_time() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "iv",
            ArrowDataType::Interval(IntervalUnit::DayTime),
            true,
        )]));
        let rows = encode(
            schema,
            vec![Arc::new(IntervalDayTimeArray::from(vec![
                Some(IntervalDayTime {
                    days: 1,
                    milliseconds: 5000,
                }),
                None,
            ]))],
        );
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn test_encode_interval_month_day_nano() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "iv",
            ArrowDataType::Interval(IntervalUnit::MonthDayNano),
            true,
        )]));
        let rows = encode(
            schema,
            vec![Arc::new(IntervalMonthDayNanoArray::from(vec![
                Some(IntervalMonthDayNano {
                    months: 1,
                    days: 2,
                    nanoseconds: 3_000_000_000,
                }),
                None,
            ]))],
        );
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn test_encode_uint32() {
        // u32::MAX (4294967295) must not truncate — widened to INT8
        let schema = Arc::new(Schema::new(vec![Field::new(
            "v",
            ArrowDataType::UInt32,
            false,
        )]));
        let rows = encode(
            schema,
            vec![Arc::new(UInt32Array::from(vec![0u32, u32::MAX]))],
        );
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn test_encode_uint16_above_i16_max() {
        // Value 50000 exceeds i16::MAX (32767) and would be silently truncated
        // if mapped to INT2. With the fix it is encoded as INT4 (i32).
        let schema = Arc::new(Schema::new(vec![Field::new(
            "v",
            ArrowDataType::UInt16,
            false,
        )]));
        let rows = encode(
            schema,
            vec![Arc::new(UInt16Array::from(vec![
                0u16,
                32767u16,
                50000u16,
                u16::MAX,
            ]))],
        );
        assert_eq!(rows.len(), 4);
        assert_eq!(arrow_type_to_pgwire(&ArrowDataType::UInt16), Type::INT4);
    }

    #[test]
    fn test_build_field_info() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", ArrowDataType::Int32, false),
            Field::new("name", ArrowDataType::Utf8, true),
            Field::new("score", ArrowDataType::Float64, true),
        ]));
        let fields = build_field_info(&schema);
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].name(), "id");
        assert_eq!(fields[1].name(), "name");
        assert_eq!(fields[2].name(), "score");
    }
}
