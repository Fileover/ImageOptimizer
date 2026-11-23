/*
 * shim_webp.c - libwebp integration (encode: WebPConfig + WebPPicture + libwebpmux;
 *               decode: WebPDecode + libwebpdemux). Upstream: webmproject/libwebp.
 */
#include "io_shim.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <webp/decode.h>
#include <webp/demux.h>
#include <webp/encode.h>
#include <webp/mux.h>
#include <webp/mux_types.h>

uint32_t shim_webp_encoder_version(void) { return (uint32_t)WebPGetEncoderVersion(); }

static void demux_chunk(WebPDemuxer *demux, const char *fourcc, io_buffer *out) {
    WebPChunkIterator it;
    if (WebPDemuxGetChunk(demux, fourcc, 1, &it)) {
        if (it.chunk.size > 0) io_buffer_set(out, it.chunk.bytes, it.chunk.size);
        WebPDemuxReleaseChunkIterator(&it);
    }
}

/* Some writers prefix the WebP EXIF chunk with "Exif\0\0"; the TIFF header is what we keep. */
static void normalize_exif(io_buffer *exif) {
    if (exif->len > 6 && memcmp(exif->data, "Exif\0\0", 6) == 0) {
        memmove(exif->data, exif->data + 6, exif->len - 6);
        exif->len -= 6;
    }
}

int shim_webp_decode(const uint8_t *data, size_t len, uint64_t max_pixels, io_image *out,
                     char *err) {
    WebPBitstreamFeatures f;
    memset(out, 0, sizeof(*out));
    VP8StatusCode st = WebPGetFeatures(data, len, &f);
    if (st != VP8_STATUS_OK) {
        io_set_err(err, "invalid or truncated WebP bitstream");
        return IO_ERR_CORRUPT;
    }
    if (f.has_animation) {
        out->is_animated = 1;
        io_set_err(err, "animated WebP is not supported (animation would be discarded)");
        return IO_ERR_UNSUPPORTED;
    }
    if (f.width <= 0 || f.height <= 0 || (uint64_t)f.width * (uint64_t)f.height > max_pixels) {
        io_set_err(err, "image dimensions exceed the configured pixel limit");
        return IO_ERR_TOO_LARGE;
    }
    int channels = f.has_alpha ? 4 : 3;
    uint8_t *px = f.has_alpha ? WebPDecodeRGBA(data, len, NULL, NULL)
                              : WebPDecodeRGB(data, len, NULL, NULL);
    if (!px) {
        io_set_err(err, "libwebp failed to decode the image");
        return IO_ERR_CORRUPT;
    }
    size_t n = (size_t)f.width * (size_t)f.height * (size_t)channels;
    out->pixels = (uint8_t *)malloc(n);
    if (!out->pixels) {
        WebPFree(px);
        io_set_err(err, "out of memory");
        return IO_ERR_OOM;
    }
    memcpy(out->pixels, px, n);
    WebPFree(px);
    out->pixels_len = n;
    out->width = (uint32_t)f.width;
    out->height = (uint32_t)f.height;
    out->channels = (uint32_t)channels;
    out->bit_depth = 8;

    WebPData wd = {data, len};
    WebPDemuxer *demux = WebPDemux(&wd);
    if (demux) {
        demux_chunk(demux, "ICCP", &out->icc);
        demux_chunk(demux, "EXIF", &out->exif);
        demux_chunk(demux, "XMP ", &out->xmp);
        normalize_exif(&out->exif);
        WebPDemuxDelete(demux);
    }
    return IO_OK;
}

static const char *enc_error_text(WebPEncodingError e) {
    switch (e) {
    case VP8_ENC_ERROR_OUT_OF_MEMORY: return "out of memory";
    case VP8_ENC_ERROR_BITSTREAM_OUT_OF_MEMORY: return "out of memory while flushing bits";
    case VP8_ENC_ERROR_NULL_PARAMETER: return "null parameter";
    case VP8_ENC_ERROR_INVALID_CONFIGURATION: return "invalid configuration";
    case VP8_ENC_ERROR_BAD_DIMENSION: return "image dimension exceeds the WebP limit (16383)";
    case VP8_ENC_ERROR_PARTITION0_OVERFLOW: return "partition #0 is too big (try lowering quality)";
    case VP8_ENC_ERROR_PARTITION_OVERFLOW: return "partition is too big (try lowering quality)";
    case VP8_ENC_ERROR_BAD_WRITE: return "bad write";
    case VP8_ENC_ERROR_FILE_TOO_BIG: return "file is too big";
    case VP8_ENC_ERROR_USER_ABORT: return "aborted";
    default: return "unknown libwebp encoder error";
    }
}

int shim_webp_encode(const io_image *in, const webp_params *p, io_buffer *out, char *err) {
    WebPConfig config;
    WebPPicture pic;
    WebPMemoryWriter writer;
    int rc = IO_ERR_GENERIC;
    memset(out, 0, sizeof(*out));

    if (in->bit_depth != 8 || (in->channels != 3 && in->channels != 4)) {
        io_set_err(err, "WebP encoder requires 8-bit RGB or RGBA input");
        return IO_ERR_UNSUPPORTED;
    }
    if (in->width > 16383 || in->height > 16383) {
        io_set_err(err, "image dimensions exceed the WebP limit of 16383 pixels");
        return IO_ERR_UNSUPPORTED;
    }
    if (!WebPConfigInit(&config)) {
        io_set_err(err, "WebPConfigInit failed (library version mismatch)");
        return IO_ERR_GENERIC;
    }
    if (p->lossless_preset >= 0) {
        if (!WebPConfigLosslessPreset(&config, p->lossless_preset)) {
            io_set_err(err, "invalid WebP lossless preset (must be 0..9)");
            return IO_ERR_GENERIC;
        }
        if (p->quality >= 0.0f) config.quality = p->quality;
        if (p->method >= 0) config.method = p->method;
    } else {
        config.lossless = p->lossless ? 1 : 0;
        config.quality = p->quality;
        config.method = p->method;
    }
    config.alpha_quality = p->alpha_quality;
    config.thread_level = p->threads > 1 ? 1 : 0;
    config.exact = p->exact ? 1 : 0;
    config.use_sharp_yuv = p->sharp_yuv ? 1 : 0;
    if (!WebPValidateConfig(&config)) {
        io_set_err(err, "invalid WebP encoder configuration (check quality/method ranges)");
        return IO_ERR_GENERIC;
    }
    if (!WebPPictureInit(&pic)) {
        io_set_err(err, "WebPPictureInit failed");
        return IO_ERR_GENERIC;
    }
    pic.width = (int)in->width;
    pic.height = (int)in->height;
    pic.use_argb = config.lossless ? 1 : 0;
    WebPMemoryWriterInit(&writer);
    pic.writer = WebPMemoryWrite;
    pic.custom_ptr = &writer;

    int stride = (int)(in->width * in->channels);
    int ok = (in->channels == 4) ? WebPPictureImportRGBA(&pic, in->pixels, stride)
                                 : WebPPictureImportRGB(&pic, in->pixels, stride);
    if (!ok) {
        io_set_err(err, "WebPPictureImport failed (out of memory)");
        rc = IO_ERR_OOM;
        goto done;
    }
    if (!WebPEncode(&config, &pic)) {
        char msg[IO_ERR_LEN];
        snprintf(msg, sizeof(msg), "libwebp encoding failed: %s", enc_error_text(pic.error_code));
        io_set_err(err, msg);
        goto done;
    }

    if (in->icc.len || in->exif.len || in->xmp.len) {
        WebPData bitstream = {writer.mem, writer.size};
        WebPMux *mux = WebPMuxCreate(&bitstream, 1);
        WebPData assembled;
        WebPDataInit(&assembled);
        if (!mux) {
            io_set_err(err, "libwebpmux could not wrap the encoded bitstream");
            goto done;
        }
        WebPMuxError me = WEBP_MUX_OK;
        if (in->icc.len) {
            WebPData d = {in->icc.data, in->icc.len};
            me = WebPMuxSetChunk(mux, "ICCP", &d, 1);
        }
        if (me == WEBP_MUX_OK && in->exif.len) {
            WebPData d = {in->exif.data, in->exif.len};
            me = WebPMuxSetChunk(mux, "EXIF", &d, 1);
        }
        if (me == WEBP_MUX_OK && in->xmp.len) {
            WebPData d = {in->xmp.data, in->xmp.len};
            me = WebPMuxSetChunk(mux, "XMP ", &d, 1);
        }
        if (me != WEBP_MUX_OK || WebPMuxAssemble(mux, &assembled) != WEBP_MUX_OK) {
            io_set_err(err, "libwebpmux failed to attach ICC/EXIF/XMP metadata");
            WebPMuxDelete(mux);
            WebPDataClear(&assembled);
            goto done;
        }
        int e = io_buffer_set(out, assembled.bytes, assembled.size);
        WebPMuxDelete(mux);
        WebPDataClear(&assembled);
        if (e) {
            io_set_err(err, "out of memory");
            rc = IO_ERR_OOM;
            goto done;
        }
    } else if (io_buffer_set(out, writer.mem, writer.size)) {
        io_set_err(err, "out of memory");
        rc = IO_ERR_OOM;
        goto done;
    }
    rc = IO_OK;
done:
    WebPPictureFree(&pic);
    WebPMemoryWriterClear(&writer);
    return rc;
}
