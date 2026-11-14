/* The C APIs SovEmu.Media binds: libvpx's VP8 encoder (and decoder, for tests and future
 * checks) and libopus. Found through pkg-config by build.rs. */
#include <vpx/vpx_encoder.h>
#include <vpx/vpx_decoder.h>
#include <vpx/vp8cx.h>
#include <vpx/vp8dx.h>
#include <opus.h>
