/* C ABI smoke program (ROADMAP item 10), driven by tests/cabi.rs.
 *
 * Usage:
 *   cabi_smoke --version     print the library version, exit 0
 *   cabi_smoke <file>        print the inspect JSON for <file> to stdout
 *   cabi_smoke --parse <file> <still-out>
 *                            print the parse JSON to stdout, write the
 *                            still bytes to <still-out>
 *   cabi_smoke --hash <file> print the coding-hash JSON to stdout
 *   cabi_smoke --aux <file>  print the auxiliary-hashes JSON to stdout
 *
 * Exit codes: 0 ok, 1 call failed, 2 usage or I/O error.
 * Invalid file bytes are NOT an error: inspect returns {"ok": false,
 * ...} JSON, parse/hash/aux return an {"error": ...} JSON.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "momentstays_core.h"

static unsigned char *read_all(const char *path, size_t *len_out) {
    FILE *f = fopen(path, "rb");
    unsigned char *buf = NULL;
    size_t len = 0, cap = 0;
    if (!f) {
        return NULL;
    }
    for (;;) {
        if (len == cap) {
            size_t ncap = cap ? cap * 2 : 65536;
            unsigned char *nbuf = (unsigned char *)realloc(buf, ncap);
            if (!nbuf) {
                free(buf);
                fclose(f);
                return NULL;
            }
            buf = nbuf;
            cap = ncap;
        }
        size_t n = fread(buf + len, 1, cap - len, f);
        len += n;
        if (n == 0) {
            break;
        }
    }
    if (ferror(f)) {
        free(buf);
        fclose(f);
        return NULL;
    }
    fclose(f);
    *len_out = len;
    return buf;
}

int main(int argc, char **argv) {
    if (argc == 2 && strcmp(argv[1], "--version") == 0) {
        printf("%s\n", momentstays_version());
        return 0;
    }
    if (argc == 4 && strcmp(argv[1], "--parse") == 0) {
        size_t len = 0;
        unsigned char *data = read_all(argv[2], &len);
        if (!data) {
            fprintf(stderr, "cannot read %s\n", argv[2]);
            return 2;
        }
        char *json = NULL;
        const uint8_t *still = NULL;
        uintptr_t still_len = 0;
        int32_t rc = momentstays_parse(data, len, &json, &still, &still_len);
        free(data);
        if (rc != 0) {
            fprintf(stderr, "parse failed: %d\n", (int)rc);
            return 1;
        }
        printf("%s\n", json);
        momentstays_free_string(json);
        FILE *o = fopen(argv[3], "wb");
        if (!o) {
            fprintf(stderr, "cannot write %s\n", argv[3]);
            return 2;
        }
        if (still_len > 0 && fwrite(still, 1, still_len, o) != still_len) {
            fclose(o);
            fprintf(stderr, "short write %s\n", argv[3]);
            return 2;
        }
        fclose(o);
        momentstays_free_bytes((uint8_t *)still, still_len);
        return 0;
    }
    if (argc == 3 && (strcmp(argv[1], "--hash") == 0 || strcmp(argv[1], "--aux") == 0)) {
        size_t len = 0;
        unsigned char *data = read_all(argv[2], &len);
        if (!data) {
            fprintf(stderr, "cannot read %s\n", argv[2]);
            return 2;
        }
        char *json = NULL;
        int32_t rc = strcmp(argv[1], "--hash") == 0
            ? momentstays_coding_hash(data, len, &json)
            : momentstays_auxiliary_hashes(data, len, &json);
        free(data);
        if (rc != 0) {
            fprintf(stderr, "hash failed: %d\n", (int)rc);
            return 1;
        }
        printf("%s\n", json);
        momentstays_free_string(json);
        return 0;
    }
    if (argc != 2) {
        fprintf(stderr, "usage: cabi_smoke [--version | <file> | --parse <file> <still-out> | --hash <file> | --aux <file>]\n");
        return 2;
    }
    size_t len = 0;
    unsigned char *data = read_all(argv[1], &len);
    if (!data) {
        fprintf(stderr, "cannot read %s\n", argv[1]);
        return 2;
    }
    char *json = NULL;
    int32_t rc = momentstays_inspect(data, len, &json);
    free(data);
    if (rc != 0) {
        fprintf(stderr, "inspect failed: %d\n", (int)rc);
        return 1;
    }
    printf("%s\n", json);
    momentstays_free_string(json);
    return 0;
}
