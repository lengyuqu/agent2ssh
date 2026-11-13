#ifndef A2S_MACH_LAB_PROTOCOL_H
#define A2S_MACH_LAB_PROTOCOL_H
// Private test protocol, intentionally not a production wire format.
#include <mach/mach.h>
#include <stdint.h>
#define A2S_LAB_HELLO 0xA221
#define A2S_LAB_HELLO_REPLY 0xA222
#define A2S_LAB_STOP 0xA223
#define A2S_LAB_STOP_REPLY 0xA224
#define A2S_LAB_TOKEN "a2s-mach-lab-synthetic-token"
typedef struct { mach_msg_header_t h; mach_msg_body_t body; mach_msg_port_descriptor_t reply; uint8_t nonce[16]; char instance[33]; uint32_t expected_pid; } a2s_lab_hello;
typedef struct { mach_msg_header_t h; mach_msg_body_t body; mach_msg_port_descriptor_t identity,endpoint; uint8_t nonce[16]; char instance[33]; uint32_t pid; } a2s_lab_hello_reply;
typedef struct { mach_msg_header_t h; mach_msg_body_t body; mach_msg_port_descriptor_t reply; uint8_t nonce[16]; char instance[33]; uint32_t expected_pid; char token[64]; } a2s_lab_stop;
typedef struct { mach_msg_header_t h; uint8_t nonce[16]; uint32_t accepted; uint32_t pid; } a2s_lab_stop_reply;
typedef union { mach_msg_header_t h; a2s_lab_hello hello; a2s_lab_stop stop; uint8_t bytes[2048]; } a2s_lab_receive;
#endif
