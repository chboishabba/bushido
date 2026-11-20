/* Debug synthetic pre-send cancellation probe; no page JS, network, or IPC
 * authority. Built separately by the operator, never shipped by default. */
#include <webkit2/webkit-web-extension.h>
#include <gmodule.h>
#include <string.h>

#define ROOT "https://bushido-tunnel.test/"
#define MAX_MESSAGES 128
#define PREFIX "bushido.fixture.policy.v2:"
#define QUERY "bushido.fixture.policy.challenge.v1"
#define REPLY "bushido.fixture.policy.reply.v1"
static gchar received[sizeof(PREFIX)+32+1+4];
static gboolean initialized = FALSE;
static gchar decisions[4] = {'X','X','X','X'};

typedef struct {
    guint sent;
    guint challenges;
    gboolean initialized;
    gchar received[sizeof(received)];
    gchar decisions[sizeof(decisions)];
} PageState;

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
        if (state->decisions[i]=='D') {
            const gchar *name = i==1 ? "bushido.fixture.v1.deny_direct" :
                i==3 ? (previous ? "bushido.fixture.v1.deny_redirect" :
                                   "bushido.fixture.v1.deny_target_direct") :
                       "bushido.fixture.v2.other_deny";
            observe(page,state,name);
            return TRUE;
        }
        if (state->decisions[i]=='A') {
            observe(page,state,i==0 ? "bushido.fixture.v1.allow_control" :
                i==2 ? "bushido.fixture.v1.allow_redirect" : "bushido.fixture.v2.other_allow");
        } else {
            observe(page,state,"bushido.fixture.v2.abstain");
        }
        break;
    }
    return FALSE;
}

static gboolean lower_hex(const gchar *s, gsize len)
{
    for (gsize i=0;i<len;i++)
        if (!g_ascii_isdigit(s[i]) && !(s[i]>='a' && s[i]<='f')) return FALSE;
    return TRUE;
}

/* Reply on the original native request/reply channel. Report only the
 * immutable parsed initialization string actually used by on_send. */
static gboolean on_challenge(WebKitWebPage *page, WebKitUserMessage *message, gpointer data)
{
    (void)page;
    PageState *state=data;
    if (g_strcmp0(webkit_user_message_get_name(message), QUERY)!=0) return FALSE;
    if (state->challenges>=32) return TRUE;
    state->challenges++;
    GVariant *param=webkit_user_message_get_parameters(message);
    if (!param || !g_variant_is_of_type(param,G_VARIANT_TYPE_STRING) ||
        webkit_user_message_get_fd_list(message)) return TRUE;
    const gchar *nonce=g_variant_get_string(param,NULL);
    if (strlen(nonce)!=32 || !lower_hex(nonce,32)) return TRUE;
    gchar *payload=g_strdup_printf("%s:%s",nonce,state->initialized?state->received:"invalid");
    webkit_user_message_send_reply(message,
        webkit_user_message_new(REPLY,g_variant_new_string(payload)));
    g_free(payload);
    return TRUE;
}

static void page_created(WebKitWebExtension *extension, WebKitWebPage *page,
                         gpointer unused)
{
    (void)extension;
    (void)unused;
    PageState *state = g_new0(PageState, 1);
    if (g_object_get_data(G_OBJECT(page),"bushido-fixture-state")) {g_free(state);return;}
    state->initialized=initialized;
    memcpy(state->received,received,sizeof(state->received));
    memcpy(state->decisions,decisions,sizeof(state->decisions));
    g_object_set_data_full(G_OBJECT(page),"bushido-fixture-state",state,g_free);
    g_signal_connect(page,"send-request",G_CALLBACK(on_send),state);
    // Both callbacks use state owned by the page, not by either signal.
    g_signal_connect(page,"user-message-received",G_CALLBACK(on_challenge),state);
    observe(page, state, "bushido.fixture.v1.page_created");
}

G_MODULE_EXPORT void webkit_web_extension_initialize_with_user_data(
    WebKitWebExtension *extension, const GVariant *user_data)
{
    /* No data or malformed data => all X; no hard-coded cancellation. */
    memset(decisions,'X',sizeof(decisions));
    memset(received,0,sizeof(received));
    initialized=FALSE;
    if (user_data && g_variant_is_of_type((GVariant *)user_data,G_VARIANT_TYPE_STRING)) {
        const gchar *s=g_variant_get_string((GVariant *)user_data,NULL);
        if (g_str_has_prefix(s,PREFIX)) {
            const gchar *generation=s+strlen(PREFIX);
            if (strlen(generation)==37 && lower_hex(generation,32) && generation[32]==':' &&
                strspn(generation+33,"ADX")==4) {
                memcpy(decisions,generation+33,4);
                g_strlcpy(received,s,sizeof(received));
                initialized=TRUE;
            }
        }
    }
    g_signal_connect(extension, "page-created", G_CALLBACK(page_created), NULL);
}
