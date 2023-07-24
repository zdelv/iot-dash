# `common`

A library of objects shared between `db-api` and `ingest`. This is mostly just
definitions for structs like `Sensor` and `Reading`. This libray helps prevent
code duplication between `db-api` and `ingest`.

## Testing

`common` contains simple tests to verify that `Sensor` and `Reading` both
serialize into the proper JSON strings (through Serde). Run these tests using:

```bash
cargo test
```
