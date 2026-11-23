/*
 * io_shim.h - Small C ABI shared by all native codec shims.
 *
 * Every codec library (libwebp, libjxl, MozJPEG/libjpeg, jpegoptim core) is wrapped by a
 * thin C shim that includes the *real upstream headers*. The Rust side therefore never
 * declares upstream structs by hand: struct layouts are always computed by the C compiler.
 * The Rust code only sees the flat types declared here.
 *
 * Ownership rules
 *  - Any pointer returned inside io_buffer / io_image is allocated with malloc() inside the
 *    shim and must be released with io_buffer_release() / io_image_release().
 *  - Input pointers are borrowed for the duration of the call only.
 *  - Every function returns 0 on success and a non-zero IO_ERR_* code otherwise. A
 *    NUL-terminated, human readable message is written to the caller supplied `err` buffer
 *    (IO_ERR_LEN bytes).
 */
#ifndef IO_SHIM_H
#define IO_SHIM_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define IO_ERR_LEN 256

#define IO_OK 0
#define IO_ERR_GENERIC 1
#define IO_ERR_UNSUPPORTED 2 /* valid input, feature not supported (animation, CMYK, ...) */
#define IO_ERR_TOO_LARGE 3   /* dimensions exceed the caller supplied limit              */
#define IO_ERR_CORRUPT 4     /* malformed input                                          */
#define IO_ERR_NO_RECON 5    /* JXL file carries no JPEG reconstruction data             */
#define IO_ERR_OOM 6

typedef struct {
    uint8_t *data;
    size_t len;
} io_buffer;

/*
 * Decoded raster. Samples are interleaved. 16-bit samples are stored as native-endian
 * uint16_t. EXIF is the raw TIFF block (starting with "II*\0" or "MM\0*"), XMP is the raw
 * XMP packet, ICC is the raw ICC profile - i.e. container specific framing is removed.
 */
typedef struct {
    uint32_t width;
    uint32_t height;
    uint32_t channels;  /* 1 gray, 2 gray+alpha, 3 RGB, 4 RGBA (4 also used for CMYK, see is_cmyk) */
    uint32_t bit_depth; /* 8 or 16 */
    uint8_t *pixels;
    size_t pixels_len;
    io_buffer icc;
    io_buffer exif;
    io_buffer xmp;
    int is_animated;
    int is_cmyk;
} io_image;

void io_free(void *p);
void io_buffer_release(io_buffer *b);
void io_image_release(io_image *img);
/* Copies [data,len) into a fresh malloc'ed buffer. Returns 0 on success. */
int io_buffer_set(io_buffer *b, const uint8_t *data, size_t len);
void io_set_err(char *err, const char *msg);

/* ------------------------------------------------------------------ libwebp */
typedef struct {
    int lossless;        /* 1 = lossless                                            */
    int lossless_preset; /* -1 = unused, 0..9 = WebPConfigLosslessPreset (cwebp -z) */
    float quality;       /* 0..100 (or <0 to keep lossless_preset quality)          */
    int method;          /* 0..6 (or <0 to keep lossless_preset method)             */
    int alpha_quality;   /* 0..100                                                  */
    int threads;         /* >1 enables libwebp's encoder thread                     */
    int exact;           /* 1 = preserve RGB values under fully transparent pixels  */
    int sharp_yuv;
} webp_params;

uint32_t shim_webp_encoder_version(void);
int shim_webp_decode(const uint8_t *data, size_t len, uint64_t max_pixels, io_image *out,
                     char *err);
int shim_webp_encode(const io_image *in, const webp_params *p, io_buffer *out, char *err);

/* ------------------------------------------------------------------- libjxl */
typedef struct {
    int lossless;       /* 1 = JxlEncoderSetFrameLossless                          */
    float distance;     /* used when lossless == 0                                 */
    int effort;         /* 1..11 (11 enables JxlEncoderAllowExpertOptions)         */
    int brotli_effort;  /* -1 = libjxl default, 0..11                              */
    int threads;
    int orientation;    /* EXIF orientation 1..8 stored in the codestream header   */
    int store_metadata; /* 1 = write Exif / XMP boxes                              */
} jxl_params;

uint32_t shim_jxl_encoder_version(void);
uint32_t shim_jxl_decoder_version(void);
float shim_jxl_distance_from_quality(float quality);
int shim_jxl_encode(const io_image *in, const jxl_params *p, io_buffer *out, char *err);
/* Lossless JPEG transcoding (JxlEncoderAddJPEGFrame with JPEG metadata stored). */
int shim_jxl_encode_jpeg(const uint8_t *jpeg, size_t len, const jxl_params *p, io_buffer *out,
                         char *err);
/* keep_orientation = 1 returns pixels exactly as stored in the codestream (not rotated). */
int shim_jxl_decode(const uint8_t *data, size_t len, uint64_t max_pixels, int threads,
                    int keep_orientation, io_image *out, char *err);
/* Returns IO_ERR_NO_RECON when the file has no JPEG reconstruction data. */
int shim_jxl_decode_jpeg(const uint8_t *data, size_t len, int threads, io_buffer *jpeg_out,
                         char *err);

/* ----------------------------------------------------------- MozJPEG decode/encode */
typedef struct {
    int quality;     /* 0..100 */
    int progressive; /* 1 = progressive scans, 0 = baseline */
} jpeg_enc_params;

const char *shim_jpeg_library_version(void);
int shim_jpeg_decode(const uint8_t *data, size_t len, uint64_t max_pixels, io_image *out,
                     char *err);
int shim_jpeg_encode(const io_image *in, const jpeg_enc_params *p, io_buffer *out, char *err);

/* ------------------------------------------------------- jpegoptim optimization core */
typedef struct {
    int quality;          /* -1 = lossless optimization; 0..100 = lossy recompression    */
    int progressive_mode; /* 0 keep input mode, 1 force progressive, 2 force baseline    */
    int strip_com;
    int strip_exif;
    int strip_iptc;
    int strip_icc;
    int strip_xmp;
    uint64_t max_pixels;
} jpegoptim_params;

typedef struct {
    uint32_t width;
    uint32_t height;
    int components;
    int progressive_in;
    int arithmetic_in;
    int saw_jfif;
    int saw_adobe;
    int markers_in;
    int markers_written;
    size_t trailing_bytes;          /* bytes after the EOI marker                          */
    int trailing_nonzero;           /* 1 if any trailing byte is non-zero                  */
} jpegoptim_info;

int shim_jpegoptim_optimize(const uint8_t *data, size_t len, const jpegoptim_params *p,
                            io_buffer *out, jpegoptim_info *info, char *err);
/*
 * Compares two JPEG files at the DCT-coefficient level (dimensions, sampling factors,
 * quantization tables and every quantized coefficient). If compare_markers is non-zero the
 * saved COM/APPn markers (excluding JFIF) must also be identical and in the same order.
 * *equal is set to 1/0. Returns non-zero only if a file could not be parsed.
 */
int shim_jpeg_coefficients_equal(const uint8_t *a, size_t alen, const uint8_t *b, size_t blen,
                                 int compare_markers, int *equal, char *err);

#ifdef __cplusplus
}
#endif
#endif /* IO_SHIM_H */
