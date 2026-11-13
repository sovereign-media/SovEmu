/*
 * libretro hands cores a printf-style, C-variadic log callback (retro_log_printf_t). Stable Rust
 * cannot define a C-variadic function, so this shim formats the message and forwards it to a
 * plain (level, message) sink that the Rust frontend registers once at startup.
 */
#include <stdarg.h>
#include <stdio.h>

#include "libretro.h"

typedef void (*sovemu_log_sink_t)(enum retro_log_level level, const char *message);

static sovemu_log_sink_t g_sink = NULL;

void sovemu_set_log_sink(sovemu_log_sink_t sink) { g_sink = sink; }

void RETRO_CALLCONV sovemu_log_printf(enum retro_log_level level, const char *fmt, ...) {
    char message[2048];
    va_list args;

    if (g_sink == NULL || fmt == NULL)
        return;

    va_start(args, fmt);
    vsnprintf(message, sizeof message, fmt, args);
    va_end(args);

    g_sink(level, message);
}
