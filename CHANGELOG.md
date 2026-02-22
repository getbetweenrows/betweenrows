# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-02-20

### ✨ Added
- **Multi Data Source Management**: The proxy now supports connecting to multiple, dynamically configured upstream data sources.
- **Data Source Admin API & UI**: New endpoints and UI pages for creating, editing, and testing data source configurations.
- **User-to-Data Source Access Control**: Implemented a many-to-many permission model to assign users to specific data sources.
- **Encryption at Rest**: Sensitive data source configuration fields (e.g., passwords) are now encrypted with AES-256-GCM in the database.
- **Engine Cache**: Implemented a cache for DataFusion `SessionContext`s, one for each active data source, to improve performance and resource management.
- **Structured Logging**: Replaced `println!` with `tracing` for structured, level-based logging.

### ♻️ Changed
- **Authentication Flow**: The PostgreSQL `database` parameter in the connection string is now used to select the target data source.
- **Project Version**: Incremented crate versions to `0.2.0` to reflect new feature set.

## [0.1.0] - (Initial Release)

- Initial implementation of the PostgreSQL wire protocol proxy.
- Authentication for proxy users via Argon2id password hashing.
- Basic query processing using the Apache DataFusion engine.
- Rudimentary admin REST API for user management.
- Initial Admin UI for listing and creating users.
