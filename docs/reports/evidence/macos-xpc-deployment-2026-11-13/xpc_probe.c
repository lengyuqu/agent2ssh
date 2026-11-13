// Isolated launchd/XPC probe. No daemon token or user data is touched.
#include <xpc/xpc.h>
#include <dispatch/dispatch.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#ifndef A2S_VARIANT
#define A2S_VARIANT "base"
#endif

static void server(const char *name, const char *client_requirement) {
    xpc_connection_t listener=xpc_connection_create_mach_service(name, dispatch_get_main_queue(), XPC_CONNECTION_MACH_SERVICE_LISTENER);
    if(!listener) exit(2);
    xpc_connection_set_event_handler(listener, ^(xpc_object_t object) {
        if(xpc_get_type(object)!=XPC_TYPE_CONNECTION) return;
        xpc_connection_t peer=(xpc_connection_t)object;
        if (client_requirement && client_requirement[0]) {
            int rc=xpc_connection_set_peer_code_signing_requirement(peer, client_requirement);
            printf("server_set_client_requirement_rc=%d\n",rc); fflush(stdout);
            if(rc) { xpc_connection_cancel(peer); return; }
        }
        xpc_connection_set_event_handler(peer, ^(xpc_object_t event) {
            if(xpc_get_type(event)!=XPC_TYPE_DICTIONARY) return;
            const char *marker=xpc_dictionary_get_string(event,"marker");
            printf("received variant=%s marker=%s\n",A2S_VARIANT,marker?marker:"none"); fflush(stdout);
            xpc_object_t reply=xpc_dictionary_create_reply(event);
            if(!reply) return;
            xpc_dictionary_set_int64(reply,"server_pid",getpid());
            xpc_dictionary_set_string(reply,"kind","synthetic-pong");
            xpc_dictionary_set_string(reply,"variant",A2S_VARIANT);
            xpc_connection_send_message(peer,reply); xpc_release(reply);
        });
        xpc_connection_activate(peer);
    });
    xpc_connection_activate(listener);
    dispatch_main();
}
static int client(const char *name,const char *requirement) {
    xpc_connection_t connection=xpc_connection_create_mach_service(name,NULL,0);
    if(!connection) return 3;
    int rc=xpc_connection_set_peer_code_signing_requirement(connection,requirement);
    printf("set_requirement_rc=%d\n",rc);
    if(rc) return 4;
    xpc_connection_set_event_handler(connection, ^(xpc_object_t event) { (void)event; });
    xpc_connection_activate(connection);
    xpc_object_t request=xpc_dictionary_create(NULL,NULL,0);
    xpc_dictionary_set_string(request,"kind","synthetic-ping");
    xpc_dictionary_set_string(request,"marker","synthetic-probe-only");
    xpc_object_t result=xpc_connection_send_message_with_reply_sync(connection,request);
    if(xpc_get_type(result)==XPC_TYPE_DICTIONARY) {
        const char *kind=xpc_dictionary_get_string(result,"kind");
        printf("result=reply kind=%s variant=%s server_pid=%lld peer_pid=%d\n",kind?kind:"missing",xpc_dictionary_get_string(result,"variant"),(long long)xpc_dictionary_get_int64(result,"server_pid"),xpc_connection_get_pid(connection));
        xpc_release(result);xpc_release(request);xpc_connection_cancel(connection);xpc_release(connection);return 0;
    }
    const char *description=xpc_dictionary_get_string(result,XPC_ERROR_KEY_DESCRIPTION);
    printf("result=error description=%s\n",description?description:"unknown");
    xpc_release(result);xpc_release(request);xpc_connection_cancel(connection);xpc_release(connection);return 5;
}
int main(int argc,char **argv) {
    setbuf(stdout,NULL);
    if((argc==3 || argc==4) && strcmp(argv[1],"server")==0) { server(argv[2],argc==4 ? argv[3] : NULL); return 0; }
    if(argc==4 && strcmp(argv[1],"client")==0) return client(argv[2],argv[3]);
    fprintf(stderr,"usage: probe server <name> | client <name> <requirement>\n");return 1;
}
