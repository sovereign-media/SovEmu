{{- define "sovemu.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "sovemu.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{- define "sovemu.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" }}
{{ include "sovemu.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{- define "sovemu.selectorLabels" -}}
app.kubernetes.io/name: {{ include "sovemu.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/* The address browsers send media to. */}}
{{- define "sovemu.advertiseIP" -}}
{{- .Values.media.advertiseIP | default .Values.media.service.loadBalancerIP }}
{{- end }}

{{- define "sovemu.lastPort" -}}
{{- add .Values.media.firstPort .Values.media.ports -1 }}
{{- end }}

{{/*
Every non-secret setting, as a dict of full variable names: what the chart derives from its
values, with `settings` on top.
*/}}
{{- define "sovemu.env" -}}
{{- $env := dict }}
{{- $_ := set $env "SOVEMU_UDP_PORTS" (printf "%d-%s" (int .Values.media.firstPort) (include "sovemu.lastPort" .)) }}
{{- $_ := set $env "SOVEMU_GPU" (toString .Values.gpu.mode) }}
{{- with include "sovemu.advertiseIP" . }}
{{- $_ := set $env "SOVEMU_ADVERTISE_IP" . }}
{{- end }}
{{- if ne .Values.library.type "none" }}
{{- $_ := set $env "SOVEMU_ROMS_DIR" (printf "/library/%s" .Values.library.romsPath) }}
{{- $_ := set $env "SOVEMU_SYSTEM_DIR" (printf "/library/%s" .Values.library.biosPath) }}
{{- end }}
{{- if .Values.ingress.tls }}
{{- $_ := set $env "SOVEMU_SECURE_COOKIES" "true" }}
{{- end }}
{{- with .Values.admin.username }}
{{- $_ := set $env "SOVEMU_ADMIN_USERNAME" . }}
{{- end }}
{{- range $i, $u := .Values.users }}
{{- $_ := set $env (printf "SOVEMU_USERS__%d__USERNAME" $i) (required "users[].username is required" $u.username) }}
{{- $_ := set $env (printf "SOVEMU_USERS__%d__ADMIN" $i) (ternary "true" "false" (default false $u.admin)) }}
{{- end }}
{{- if .Values.pruneUsers }}
{{- $_ := set $env "SOVEMU_USERS_PRUNE" "true" }}
{{- end }}
{{- range $id, $s := .Values.systems }}
{{- if $s.core }}
{{- $_ := set $env (printf "SOVEMU_SYSTEMS__%s__CORE" $id) $s.core }}
{{- end }}
{{- if hasKey $s "enabled" }}
{{- $_ := set $env (printf "SOVEMU_SYSTEMS__%s__ENABLED" $id) (ternary "true" "false" $s.enabled) }}
{{- end }}
{{- range $k, $v := $s.options }}
{{- $_ := set $env (printf "SOVEMU_SYSTEMS__%s__OPTIONS__%s" $id $k) (toString $v) }}
{{- end }}
{{- end }}
{{- range $k, $v := .Values.settings }}
{{- $_ := set $env (printf "SOVEMU_%s" (trimPrefix "SOVEMU_" $k)) (toString $v) }}
{{- end }}
{{- toYaml $env }}
{{- end }}

{{/* Secrets this chart stores, as a dict of full variable names. */}}
{{- define "sovemu.secretEnv" -}}
{{- $env := dict }}
{{- range $k, $v := .Values.secrets }}
{{- $_ := set $env (printf "SOVEMU_%s" (trimPrefix "SOVEMU_" $k)) (toString $v) }}
{{- end }}
{{- /* TURN servers carry credentials. */}}
{{- with .Values.iceServers }}
{{- $_ := set $env "SOVEMU_ICE_SERVERS" (toJson .) }}
{{- end }}
{{- with .Values.admin.password }}
{{- $_ := set $env "SOVEMU_ADMIN_PASSWORD" . }}
{{- end }}
{{- range $i, $u := .Values.users }}
{{- with $u.password }}
{{- $_ := set $env (printf "SOVEMU_USERS__%d__PASSWORD" $i) . }}
{{- end }}
{{- end }}
{{- toYaml $env }}
{{- end }}
