{{- define "swiftpipe.name" -}}
{{- .Chart.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "swiftpipe.fullname" -}}
{{- if .Release.Name -}}
{{- printf "%s-%s" .Release.Name (include "swiftpipe.name" .) | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- include "swiftpipe.name" . -}}
{{- end -}}
{{- end -}}

{{- define "swiftpipe.labels" -}}
app.kubernetes.io/name: {{ include "swiftpipe.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ .Chart.Name }}-{{ .Chart.Version | replace "+" "_" }}
{{- end -}}

{{- define "swiftpipe.selectorLabels" -}}
app.kubernetes.io/name: {{ include "swiftpipe.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}
