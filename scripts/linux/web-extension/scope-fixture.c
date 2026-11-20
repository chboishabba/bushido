/* Debug synthetic pre-send cancellation probe; no page JS, network, or IPC
 * authority. Built separately by the operator, never shipped by default. */
#include <webkit2/webkit-web-extension.h>
#include <gmodule.h>
#include <string.h>

#define ROOT "https://bushido-tunnel.test/"
#define MAX_MESSAGES 128

typedef struct { guint sent; } PageState;

static void observe(WebKitWebPage *page, PageState *state, const gchar *name)
{
    if (state->sent >= MAX_MESSAGES)
        return;
    state->sent++;
    /* Floating message is consumed by WebKit. No parameters, FDs,
     * URI, headers or credentials. No reply or execution capability. */
    webkit_web_page_send_message_to_view(page,
        webkit_user_message_new(name, NULL), NULL, NULL, NULL);
}

static gboolean on_send(WebKitWebPage *page, WebKitURIRequest *request,
                        WebKitURIResponse *previous, gpointer data)
{
    PageState *state = data;
    const gchar *uri = webkit_uri_request_get_uri(request);
    if (!uri)
        return FALSE;
    if (strcmp(uri, ROOT "extension-deny") == 0) {
        observe(page, state, "bushido.fixture.v1.deny_direct");
        return TRUE;
    }
    if (strcmp(uri, ROOT "extension-redirect-deny-target") == 0) {
        /* Count direct and redirect sends distinctly. Both fixed target
         * sends are cancelled, regardless of the optional prior response. */
        observe(page, state, previous ?
            "bushido.fixture.v1.deny_redirect" :
            "bushido.fixture.v1.deny_target_direct");
        return TRUE;
    }
    if (strcmp(uri, ROOT "extension-allow") == 0)
        observe(page, state, "bushido.fixture.v1.allow_control");
    if (strcmp(uri, ROOT "extension-redirect-deny") == 0)
        observe(page, state, "bushido.fixture.v1.allow_redirect");
    return FALSE;
}

static void free_state(gpointer state, GClosure *closure)
{
    (void)closure;
    g_free(state);
}

static void page_created(WebKitWebExtension *extension, WebKitWebPage *page,
                         gpointer unused)
{
    (void)extension;
    (void)unused;
    PageState *state = g_new0(PageState, 1);
    g_signal_connect_data(page, "send-request", G_CALLBACK(on_send), state,
                          free_state, 0);
    observe(page, state, "bushido.fixture.v1.page_created");
}

G_MODULE_EXPORT void webkit_web_extension_initialize(WebKitWebExtension *extension)
{
    g_signal_connect(extension, "page-created", G_CALLBACK(page_created), NULL);
}
