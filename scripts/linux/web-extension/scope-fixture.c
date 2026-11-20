/* Debug synthetic pre-send cancellation probe; no page JS, network, or IPC
 * authority. Built separately by the operator, never shipped by default. */
#include <webkit2/webkit-web-extension.h>
#include <gmodule.h>
#include <string.h>

#define ROOT "https://bushido-tunnel.test/"
#define MAX_MESSAGES 128
#define PREFIX "bushido.fixture.policy.v1:"
static gchar decisions[4] = {'X','X','X','X'};

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
    static const gchar *routes[4] = {
        ROOT "extension-allow", ROOT "extension-deny",
        ROOT "extension-redirect-deny", ROOT "extension-redirect-deny-target"
    };
    for (guint i=0;i<4;i++) {
        if (strcmp(uri,routes[i])!=0) continue;
        if (decisions[i]=='D') {
            const gchar *name = i==1 ? "bushido.fixture.v1.deny_direct" :
                i==3 ? (previous ? "bushido.fixture.v1.deny_redirect" :
                                   "bushido.fixture.v1.deny_target_direct") :
                       "bushido.fixture.v2.other_deny";
            observe(page,state,name);
            return TRUE;
        }
        if (decisions[i]=='A') {
            observe(page,state,i==0 ? "bushido.fixture.v1.allow_control" :
                i==2 ? "bushido.fixture.v1.allow_redirect" : "bushido.fixture.v2.other_allow");
        } else {
            observe(page,state,"bushido.fixture.v2.abstain");
        }
        break;
    }
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

G_MODULE_EXPORT void webkit_web_extension_initialize_with_user_data(
    WebKitWebExtension *extension, const GVariant *user_data)
{
    /* No data or malformed data => all X; no hard-coded cancellation. */
    if (user_data && g_variant_is_of_type((GVariant *)user_data,G_VARIANT_TYPE_STRING)) {
        const gchar *s=g_variant_get_string((GVariant *)user_data,NULL);
        if (g_str_has_prefix(s,PREFIX)) {
            const gchar *table=s+strlen(PREFIX);
            if (strlen(table)==4 && strspn(table,"ADX")==4)
                memcpy(decisions,table,4);
        }
    }
    g_signal_connect(extension, "page-created", G_CALLBACK(page_created), NULL);
}
