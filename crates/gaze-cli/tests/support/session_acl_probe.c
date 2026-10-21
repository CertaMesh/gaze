#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/acl.h>
#include <sys/stat.h>

// Interpose the real descriptor operations so a post-write permission fix fails.
static void record_empty_file(int fd, const char *event) {
    struct stat st;
    if (fstat(fd, &st) != 0 || st.st_size != 0) {
        abort();
    }
    FILE *log = fopen(getenv("GAZE_ACL_PROBE_LOG"), "a");
    if (!log || fputs(event, log) < 0 || fclose(log) != 0) {
        abort();
    }
}

static int probe_set(int fd, acl_t acl) {
    record_empty_file(fd, "clear-before-write\n");
    if (getenv("GAZE_ACL_PROBE_DENY_CLEAR")) {
        errno = EPERM;
        return -1;
    }
    if (getenv("GAZE_ACL_PROBE_KEEP_ACL")) {
        return 0;
    }
    return acl_set_fd(fd, acl);
}

static acl_t probe_get(int fd) {
    record_empty_file(fd, "verify-before-write\n");
    if (getenv("GAZE_ACL_PROBE_DENY_VERIFY")) {
        errno = EPERM;
        return NULL;
    }
    return acl_get_fd(fd);
}

__attribute__((used, section("__DATA,__interpose")))
static const struct { const void *replacement; const void *original; } hooks[] = {
    { (const void *)probe_set, (const void *)acl_set_fd },
    { (const void *)probe_get, (const void *)acl_get_fd },
};
