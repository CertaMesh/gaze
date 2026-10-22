#include <dirent.h>
#include <errno.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/acl.h>
#include <sys/stat.h>

// Interpose the real descriptor operations so a post-write permission fix fails.
static void record_empty_file(int fd, const char *event) {
    struct stat st;
    if (fstat(fd, &st) != 0 || (S_ISREG(st.st_mode) && st.st_size != 0)) {
        abort();
    }
    if (S_ISDIR(st.st_mode)) {
        DIR *directory = fdopendir(dup(fd));
        if (!directory) abort();
        struct dirent *entry;
        while ((entry = readdir(directory))) {
            if (strcmp(entry->d_name, ".") && strcmp(entry->d_name, "..")) abort();
        }
        if (closedir(directory)) abort();
    }
    FILE *log = fopen(getenv("GAZE_ACL_PROBE_LOG"), "a");
    if (!log || fputs(event, log) < 0 || fclose(log) != 0) {
        abort();
    }
}

static int retained_reader = -1;

static void try_reader(int fd) {
    char path[4096];
    if (fcntl(fd, F_GETPATH, path) != 0 || fchmod(fd, 0) != 0) abort();
    // Remove owner mode access: a successful open now relies on inherited ACLs.
    retained_reader = open(path, O_RDONLY);
    int error = errno;
    if (fchmod(fd, 0600) != 0) abort();
    if (retained_reader < 0 && error != EACCES) abort();
    record_empty_file(fd, retained_reader < 0 ? "reader-denied\n" : "reader-opened\n");
}

__attribute__((destructor))
static void finish_reader(void) {
    if (retained_reader < 0) return;
    char bytes[4096];
    ssize_t count = read(retained_reader, bytes, sizeof(bytes));
    FILE *log = fopen(getenv("GAZE_ACL_PROBE_LOG"), "a");
    if (!log || count <= 0 || fputs("retained-reader-recovered-session\n", log) < 0 || fclose(log)) abort();
    close(retained_reader);
}

static int probe_set(int fd, acl_t acl) {
    struct stat st;
    if (fstat(fd, &st) != 0) abort();
    if (S_ISREG(st.st_mode) && getenv("GAZE_ACL_PROBE_RETAIN_READER")) try_reader(fd);
    record_empty_file(fd, S_ISDIR(st.st_mode) ? "clear-directory\n" : "clear-before-write\n");
    if (getenv("GAZE_ACL_PROBE_FILE_ONLY") && S_ISDIR(st.st_mode)) return acl_set_fd(fd, acl);
    if (getenv("GAZE_ACL_PROBE_DENY_CLEAR")) {
        errno = EPERM;
        return -1;
    }
    if (getenv("GAZE_ACL_PROBE_KEEP_ACL")) {
        if (S_ISREG(st.st_mode)) {
            acl_t inherited = acl_get_file(getenv("GAZE_ACL_PROBE_PARENT"), ACL_TYPE_EXTENDED);
            if (!inherited || acl_set_fd(fd, inherited) != 0 || acl_free(inherited) != 0) abort();
        }
        return 0;
    }
    return acl_set_fd(fd, acl);
}

static acl_t probe_get(int fd) {
    struct stat st;
    if (fstat(fd, &st) != 0) abort();
    record_empty_file(fd, S_ISDIR(st.st_mode) ? "verify-directory\n" : "verify-before-write\n");
    if (getenv("GAZE_ACL_PROBE_DENY_VERIFY") &&
        (!getenv("GAZE_ACL_PROBE_FILE_ONLY") || S_ISREG(st.st_mode))) {
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
