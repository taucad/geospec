/*
 * V1c measurement shim: counts durability syncs and, built with -DV1C_NOSYNC, turns them into no-ops.
 *
 * Covers fsync, fdatasync and fcntl(F_FULLFSYNC / F_BARRIERFSYNC). Rust's File::sync_all and libuv's fsync
 * issue fcntl(fd, F_FULLFSYNC) on Apple, so fsync alone would miss them.
 *   macOS: DYLD_INSERT_LIBRARIES=<dylib> (the target must not be a SIP-protected binary).
 *   Linux: LD_PRELOAD=<so> (driver testing only; there is no F_FULLFSYNC).
 * Each process writes its cumulative counts to $V1C_SYNC_LOG/<pid>.tsv (rewritten at most every 200 ms and
 * at exit): fsync_n fsync_ns fdatasync_n fdatasync_ns fullfsync_n fullfsync_ns barrierfsync_n barrierfsync_ns.
 * Measurement only; never ship.
 */
#define _GNU_SOURCE
#include <fcntl.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#ifdef __APPLE__
#ifndef F_BARRIERFSYNC
#define F_BARRIERFSYNC 85
#endif
#else
#include <dlfcn.h>
#define F_FULLFSYNC (-1001)
#define F_BARRIERFSYNC (-1002)
#endif

enum { K_FSYNC, K_FDATASYNC, K_FULL, K_BARRIER, K_N };
static uint64_t counts[K_N], nanos[K_N];
static uint64_t last_flush_ns;

static uint64_t now_ns(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
}

static void flush_counts(void) {
  const char *dir = getenv("V1C_SYNC_LOG");
  if (!dir || !*dir) return;
  char path[1024], line[512];
  snprintf(path, sizeof path, "%s/%d.tsv", dir, (int)getpid());
  int n = snprintf(line, sizeof line, "%llu\t%llu\t%llu\t%llu\t%llu\t%llu\t%llu\t%llu\n",
                   (unsigned long long)__atomic_load_n(&counts[K_FSYNC], __ATOMIC_RELAXED),
                   (unsigned long long)__atomic_load_n(&nanos[K_FSYNC], __ATOMIC_RELAXED),
                   (unsigned long long)__atomic_load_n(&counts[K_FDATASYNC], __ATOMIC_RELAXED),
                   (unsigned long long)__atomic_load_n(&nanos[K_FDATASYNC], __ATOMIC_RELAXED),
                   (unsigned long long)__atomic_load_n(&counts[K_FULL], __ATOMIC_RELAXED),
                   (unsigned long long)__atomic_load_n(&nanos[K_FULL], __ATOMIC_RELAXED),
                   (unsigned long long)__atomic_load_n(&counts[K_BARRIER], __ATOMIC_RELAXED),
                   (unsigned long long)__atomic_load_n(&nanos[K_BARRIER], __ATOMIC_RELAXED));
  int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
  if (fd < 0) return;
  ssize_t w = write(fd, line, (size_t)n);
  (void)w;
  close(fd);
}

static void note(int kind, uint64_t t0) {
  uint64_t t1 = now_ns();
  __atomic_fetch_add(&counts[kind], 1, __ATOMIC_RELAXED);
  __atomic_fetch_add(&nanos[kind], t1 - t0, __ATOMIC_RELAXED);
  uint64_t last = __atomic_load_n(&last_flush_ns, __ATOMIC_RELAXED);
  if (t1 - last > 200000000ull &&
      __atomic_compare_exchange_n(&last_flush_ns, &last, t1, 0, __ATOMIC_RELAXED, __ATOMIC_RELAXED))
    flush_counts();
}

__attribute__((constructor)) static void v1c_init(void) { flush_counts(); }
__attribute__((destructor)) static void v1c_fini(void) { flush_counts(); }

#ifdef __APPLE__
/* dyld interposition: calls from inside this image reach the originals. */
#define REAL(name) name
int v1c_fsync(int fd);
int v1c_fdatasync(int fd);
int v1c_fcntl(int fd, int cmd, ...);
#ifndef V1C_NO_FDATASYNC
extern int fdatasync(int fd);
#endif
#define INTERPOSE(replacement, original)                                                             \
  __attribute__((used)) static struct {                                                              \
    const void *r;                                                                                   \
    const void *o;                                                                                   \
  } interpose_##original __attribute__((section("__DATA,__interpose"))) = {(const void *)&replacement, \
                                                                           (const void *)&original};
INTERPOSE(v1c_fsync, fsync)
#ifndef V1C_NO_FDATASYNC
INTERPOSE(v1c_fdatasync, fdatasync)
#endif
INTERPOSE(v1c_fcntl, fcntl)
#define FSYNC_NAME v1c_fsync
#define FDATASYNC_NAME v1c_fdatasync
#define FCNTL_NAME v1c_fcntl
static int real_fsync(int fd) { return fsync(fd); }
#ifndef V1C_NO_FDATASYNC
static int real_fdatasync(int fd) { return fdatasync(fd); }
#else
static int real_fdatasync(int fd) { return fsync(fd); }
#endif
static int real_fcntl(int fd, int cmd, intptr_t arg) { return fcntl(fd, cmd, arg); }
#else
#define FSYNC_NAME fsync
#define FDATASYNC_NAME fdatasync
static int real_fsync(int fd) {
  static int (*f)(int);
  if (!f) f = (int (*)(int))dlsym(RTLD_NEXT, "fsync");
  return f(fd);
}
static int real_fdatasync(int fd) {
  static int (*f)(int);
  if (!f) f = (int (*)(int))dlsym(RTLD_NEXT, "fdatasync");
  return f(fd);
}
#endif

int FSYNC_NAME(int fd) {
  uint64_t t0 = now_ns();
#ifdef V1C_NOSYNC
  (void)fd;
  int r = 0;
#else
  int r = real_fsync(fd);
#endif
  note(K_FSYNC, t0);
  return r;
}

int FDATASYNC_NAME(int fd) {
  uint64_t t0 = now_ns();
#ifdef V1C_NOSYNC
  (void)fd;
  int r = 0;
#else
  int r = real_fdatasync(fd);
#endif
  note(K_FDATASYNC, t0);
  return r;
}

#ifdef __APPLE__
int FCNTL_NAME(int fd, int cmd, ...) {
  va_list ap;
  va_start(ap, cmd);
  intptr_t arg = va_arg(ap, intptr_t);
  va_end(ap);
  if (cmd != F_FULLFSYNC && cmd != F_BARRIERFSYNC) return real_fcntl(fd, cmd, arg);
  uint64_t t0 = now_ns();
#ifdef V1C_NOSYNC
  int r = 0;
#else
  int r = real_fcntl(fd, cmd, arg);
#endif
  note(cmd == F_FULLFSYNC ? K_FULL : K_BARRIER, t0);
  return r;
}
#endif
