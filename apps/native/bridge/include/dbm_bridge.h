#ifndef DBM_BRIDGE_H
#define DBM_BRIDGE_H

#include <stddef.h>
#include <stdint.h>

typedef struct DbmBridgeSession DbmBridgeSession;

/* Sessions are thread-safe and serialize calls internally. The caller owns a
 * session until free, and must not free it concurrently with a call. On create
 * failure, error_out receives an owned UTF-8 diagnostic when non-null. */
DbmBridgeSession *dbm_bridge_session_create(char **error_out);
/* An isolated in-memory fixture for --demo: no profiles, credentials,
 * network, or disk. Null only if the runtime cannot start. */
DbmBridgeSession *dbm_bridge_demo_session_create(void);
char *dbm_bridge_session_call(DbmBridgeSession *session,
                              const uint8_t *request, size_t length);
void dbm_bridge_session_free(DbmBridgeSession *session);

/* Editor/grid helpers (highlight, executionTarget, parseCell, csv, ...) that
 * need no session or database. Safe to call from any thread at any time. */
char *dbm_bridge_helper_call(const uint8_t *request, size_t length);

/* Every non-null response/init error is UTF-8, NUL-terminated, library-owned, and
 * must be released exactly once with this function. Null is accepted. */
void dbm_bridge_response_free(char *response);

#endif
