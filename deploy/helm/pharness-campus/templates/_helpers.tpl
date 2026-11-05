{{- define "pharness-campus.labels" -}}
app.kubernetes.io/part-of: pharness-campus
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}
