{{- /* 密钥 Secret 名: existingSecret 优先, 否则 chart 自建 */ -}}
{{- define "epicode.secretName" -}}
{{- if .Values.externalSecret.existingSecret -}}
{{- .Values.externalSecret.existingSecret -}}
{{- else -}}
{{- .Release.Name }}-secrets
{{- end -}}
{{- end -}}

{{- /* 镜像引用: digest 固定优先(不可变, 审计 2026-09 中优 #14), 否则 tag */ -}}
{{- define "epicode.image" -}}
{{- $img := .img -}}
{{- if $img.digest -}}
{{- printf "%s@%s" $img.repository $img.digest -}}
{{- else -}}
{{- printf "%s:%s" $img.repository ( $img.tag | default "latest" ) -}}
{{- end -}}
{{- end -}}
