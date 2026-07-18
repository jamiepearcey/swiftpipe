# Compose Observability Stack

Use the observability overlay with the local Compose stack when you want Grafana
with Loki and Tempo available beside the API and Postgres.

The overlay adds Loki, Tempo, Promtail, and Grafana datasource provisioning.
SwiftPipe API JSON logs are shipped to Loki through Promtail. Tempo is exposed
as an OTLP receiver and Grafana datasource for trace demonstrations once a
runtime exports traces.

From `repo/`, start the stack:

```bash
SWIFTPIPE_API_PORT=18084 SWIFTPIPE_POSTGRES_PORT=15434 SWIFTPIPE_GRAFANA_PORT=13001 SWIFTPIPE_LOKI_PORT=13100 SWIFTPIPE_TEMPO_PORT=13200 docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.observability.yml -p swiftpipe330 up -d
```

Validate local endpoints:

```bash
curl -fsS http://127.0.0.1:18084/healthz
curl -fsS http://127.0.0.1:13001/api/health
curl -fsS http://127.0.0.1:13100/ready
curl -fsS http://127.0.0.1:13200/ready
```

Stop and remove local volumes:

```bash
SWIFTPIPE_API_PORT=18084 SWIFTPIPE_POSTGRES_PORT=15434 SWIFTPIPE_GRAFANA_PORT=13001 SWIFTPIPE_LOKI_PORT=13100 SWIFTPIPE_TEMPO_PORT=13200 docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.observability.yml -p swiftpipe330 down -v
```
