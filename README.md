# swiftpipe

swiftpipe is a schema-driven SWIFT FIN ingestion engine. It is intended to parse, validate, normalize, and render financial messages into structures that are easier to store, inspect, and process than raw message traffic alone.

What is interesting about the project is its schema-centric approach. Rather than hard-coding one narrow parser path, it uses shared message descriptions as the control layer for ingestion, validation, and rendering.

## Folder guide

- `repo/`: main implementation
- `notes/`: architecture notes, planning, and working context
- `research/`: experiments, schema work, and technical exploration
