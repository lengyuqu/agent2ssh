// Test-only: send a public hello to an isolated daemon Unix socket, then use
// its kernel peer audit token for dynamic code validation. Never reads a token.
#include <mach/mach.h>
#include <Security/Security.h>
#include <CoreFoundation/CoreFoundation.h>
#include <bsm/libbsm.h>
#include <arpa/inet.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <sys/types.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static int full_read(int fd,void *p,size_t n){char *b=p;while(n){ssize_t k=read(fd,b,n);if(k<=0)return -1;b+=k;n-=(size_t)k;}return 0;}
int main(int argc,char **argv){
 if(argc!=4)return 1;int expected=atoi(argv[2]);int fd=socket(AF_UNIX,SOCK_STREAM,0);if(fd<0)return 2;
 struct sockaddr_un a={0};a.sun_family=AF_UNIX;if(strlcpy(a.sun_path,argv[1],sizeof(a.sun_path))>=sizeof(a.sun_path))return 2;
 struct timeval timeout={.tv_sec=3};if(setsockopt(fd,SOL_SOCKET,SO_RCVTIMEO,&timeout,sizeof(timeout)))return 2;
 if(connect(fd,(struct sockaddr*)&a,sizeof(a)))return 2;
 const char hello[]="{\"type\":\"hello\",\"version\":1,\"client_nonce\":\"0123456789abcdef0123456789abcdef\"}";
 uint32_t n=htonl((uint32_t)(sizeof(hello)-1));if(write(fd,&n,4)!=4||write(fd,hello,sizeof(hello)-1)!=(ssize_t)(sizeof(hello)-1))return 2;
 if(full_read(fd,&n,4))return 2;n=ntohl(n);if(n==0||n>16384)return 2;char *body=calloc(n+1,1);if(!body)return 2;if(full_read(fd,body,n))return 2;
 if(!strstr(body,"hello_ok")){free(body);return 2;}free(body);
 audit_token_t audit={0};socklen_t length=sizeof(audit);if(getsockopt(fd,SOL_LOCAL,LOCAL_PEERTOKEN,&audit,&length)||length!=sizeof(audit))return 2;close(fd);
 CFDataRef data=CFDataCreate(NULL,(const UInt8*)&audit,sizeof(audit));const void *keys[]={kSecGuestAttributeAudit},*values[]={data};
 CFDictionaryRef attrs=CFDictionaryCreate(NULL,keys,values,1,&kCFTypeDictionaryKeyCallBacks,&kCFTypeDictionaryValueCallBacks);SecCodeRef code=NULL;
 OSStatus result=SecCodeCopyGuestWithAttributes(NULL,attrs,kSecCSDefaultFlags,&code);
 OSStatus info_result=-1;int64_t flags=-1,status=-1;int dyld_entitlement=0,disable_library_validation=0;
 if(!result){CFStringRef text=CFStringCreateWithCString(NULL,argv[3],kCFStringEncodingUTF8);SecRequirementRef req=NULL;result=SecRequirementCreateWithString(text,kSecCSDefaultFlags,&req);if(!result){result=SecCodeCheckValidity(code,kSecCSDefaultFlags,req);CFRelease(req);}CFRelease(text);
  CFDictionaryRef info=NULL;info_result=SecCodeCopySigningInformation(code,kSecCSSigningInformation|kSecCSDynamicInformation,&info);
  if(!info_result && info){CFNumberRef f=CFDictionaryGetValue(info,kSecCodeInfoFlags),s=CFDictionaryGetValue(info,kSecCodeInfoStatus);if(f && CFGetTypeID(f)==CFNumberGetTypeID())CFNumberGetValue(f,kCFNumberSInt64Type,&flags);if(s && CFGetTypeID(s)==CFNumberGetTypeID())CFNumberGetValue(s,kCFNumberSInt64Type,&status);
   CFDictionaryRef ent=CFDictionaryGetValue(info,kSecCodeInfoEntitlementsDict);if(ent && CFGetTypeID(ent)==CFDictionaryGetTypeID()) {CFTypeRef d=CFDictionaryGetValue(ent,CFSTR("com.apple.security.cs.allow-dyld-environment-variables")),v=CFDictionaryGetValue(ent,CFSTR("com.apple.security.cs.disable-library-validation"));dyld_entitlement=d && CFGetTypeID(d)==CFBooleanGetTypeID() && CFBooleanGetValue(d);disable_library_validation=v && CFGetTypeID(v)==CFBooleanGetTypeID() && CFBooleanGetValue(v);}CFRelease(info);}CFRelease(code);}
 CFRelease(attrs);CFRelease(data);
 printf("peer_pid=%d pidversion=%d expected_pid=%d code_status=%d signing_info_status=%d static_flags=%lld dynamic_status=%lld runtime_flag=%d dyld_entitlement=%d disable_library_validation=%d\n",audit_token_to_pid(audit),audit_token_to_pidversion(audit),expected,(int)result,(int)info_result,(long long)flags,(long long)status,flags>=0 && !!(flags & kSecCodeSignatureRuntime),dyld_entitlement,disable_library_validation);
 return audit_token_to_pid(audit)==expected && result==errSecSuccess ? 0 : 3;
}
