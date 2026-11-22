/* The C APIs SovEmu.Media binds: libvpx's VP8 encoder (and decoder, for tests and future
 * checks), libopus, and FFmpeg's libavcodec for H.264 (its hardware encoders, VA-API, NVENC
 * and VideoToolbox, and x264; its decoder for tests). Found through pkg-config by build.rs. */
#include <vpx/vpx_encoder.h>
#include <vpx/vpx_decoder.h>
#include <vpx/vp8cx.h>
#include <vpx/vp8dx.h>
#include <opus.h>
#include <errno.h>
#include <libavcodec/avcodec.h>
#include <libavutil/hwcontext.h>
#include <libavutil/opt.h>
#include <libavutil/error.h>
