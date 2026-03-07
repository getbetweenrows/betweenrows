import type { User, LoginResponse, PaginatedResponse } from '../types/user'
import type { DataSource, DataSourceType, FieldDef } from '../types/datasource'
import type { PolicyResponse, PolicyAssignmentResponse, ObligationResponse } from '../types/policy'
import type {
  CatalogResponse,
  DiscoveredSchemaResponse,
  DiscoveredTableResponse,
  DiscoveredColumnResponse,
} from '../types/catalog'

let counter = 0
const id = () => `id-${++counter}`

export function makeUser(overrides: Partial<User> = {}): User {
  return {
    id: id(),
    username: `user_${counter}`,
    tenant: 'default',
    is_admin: false,
    is_active: true,
    email: null,
    display_name: null,
    last_login_at: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
    ...overrides,
  }
}

export function makeLoginResponse(overrides: Partial<LoginResponse> = {}): LoginResponse {
  return {
    token: 'test-token-abc',
    user: makeUser({ is_admin: true }),
    ...overrides,
  }
}

export function makePaginatedUsers(users: User[]): PaginatedResponse<User> {
  return { data: users, total: users.length, page: 1, page_size: 20 }
}

export function makeField(overrides: Partial<FieldDef> = {}): FieldDef {
  return {
    key: 'host',
    label: 'Host',
    field_type: 'text',
    required: true,
    is_secret: false,
    default_value: 'localhost',
    ...overrides,
  }
}

export function makeDataSourceType(overrides: Partial<DataSourceType> = {}): DataSourceType {
  return {
    ds_type: 'postgres',
    label: 'PostgreSQL',
    fields: [
      makeField({ key: 'host', label: 'Host', default_value: 'localhost' }),
      makeField({ key: 'port', label: 'Port', field_type: 'number', default_value: '5432' }),
      makeField({ key: 'db', label: 'Database', default_value: '' }),
      makeField({ key: 'user', label: 'User', default_value: 'postgres' }),
      makeField({ key: 'pass', label: 'Password', is_secret: true, required: false }),
    ],
    ...overrides,
  }
}

export function makeDataSource(overrides: Partial<DataSource> = {}): DataSource {
  return {
    id: id(),
    name: `datasource_${counter}`,
    ds_type: 'postgres',
    config: { host: 'localhost', port: 5432, db: 'mydb', user: 'postgres' },
    is_active: true,
    access_mode: 'policy_required',
    last_sync_at: null,
    last_sync_result: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
    ...overrides,
  }
}

export function makeDiscoveredSchema(overrides: Partial<DiscoveredSchemaResponse> = {}): DiscoveredSchemaResponse {
  return {
    schema_name: `schema_${id()}`,
    schema_alias: null,
    is_already_selected: false,
    ...overrides,
  }
}

export function makeDiscoveredTable(overrides: Partial<DiscoveredTableResponse> = {}): DiscoveredTableResponse {
  return {
    schema_name: 'public',
    table_name: `table_${id()}`,
    table_type: 'TABLE',
    is_already_selected: false,
    ...overrides,
  }
}

export function makeDiscoveredColumn(overrides: Partial<DiscoveredColumnResponse> = {}): DiscoveredColumnResponse {
  return {
    schema_name: 'public',
    table_name: 'users',
    column_name: `col_${id()}`,
    ordinal_position: counter,
    data_type: 'integer',
    is_nullable: false,
    column_default: null,
    arrow_type: 'Int64',
    is_already_selected: false,
    ...overrides,
  }
}

export function makeEmptyCatalog(): CatalogResponse {
  return { schemas: [] }
}

export function makeObligation(overrides: Partial<ObligationResponse> = {}): ObligationResponse {
  return {
    id: id(),
    obligation_type: 'row_filter',
    definition: { filter: "tenant_id = '{user.tenant}'" },
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
    ...overrides,
  }
}

export function makePolicyAssignment(
  overrides: Partial<PolicyAssignmentResponse> = {},
): PolicyAssignmentResponse {
  return {
    id: id(),
    policy_id: id(),
    policy_name: `policy_${counter}`,
    data_source_id: id(),
    datasource_name: `datasource_${counter}`,
    user_id: null,
    username: null,
    priority: 100,
    created_at: '2024-01-01T00:00:00Z',
    ...overrides,
  }
}

export function makePolicy(overrides: Partial<PolicyResponse> = {}): PolicyResponse {
  return {
    id: id(),
    name: `policy_${counter}`,
    description: null,
    effect: 'permit',
    is_enabled: true,
    version: 1,
    obligation_count: 0,
    assignment_count: 0,
    obligations: [],
    assignments: [],
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
    ...overrides,
  }
}
