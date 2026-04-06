#pragma once

#include <Arduino.h>
#ifdef RUNE_ARDUINO_ENABLE_SERVO
#include <Servo.h>
#include <new>
#endif
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#ifndef RUNE_INPUT_BUFFER_SIZE
#define RUNE_INPUT_BUFFER_SIZE 128
#endif

#ifndef RUNE_STRING_SLOT_COUNT
#define RUNE_STRING_SLOT_COUNT 8
#endif

#ifndef RUNE_STRING_SLOT_SIZE
#define RUNE_STRING_SLOT_SIZE 96
#endif

#ifndef RUNE_ARDUINO_ENABLE_STRING_RUNTIME
#define RUNE_ARDUINO_ENABLE_STRING_RUNTIME 1
#endif

#ifndef RUNE_ARDUINO_ENABLE_DYNAMIC_RUNTIME
#define RUNE_ARDUINO_ENABLE_DYNAMIC_RUNTIME 1
#endif

#ifndef RUNE_ARDUINO_ENABLE_SYSTEM_RUNTIME
#define RUNE_ARDUINO_ENABLE_SYSTEM_RUNTIME 1
#endif

#ifndef RUNE_ARDUINO_ENABLE_ENV_RUNTIME
#define RUNE_ARDUINO_ENABLE_ENV_RUNTIME 1
#endif

#ifndef RUNE_ARDUINO_ENABLE_SERIAL_WRAPPER_RUNTIME
#define RUNE_ARDUINO_ENABLE_SERIAL_WRAPPER_RUNTIME 1
#endif

#ifndef RUNE_ARDUINO_ENABLE_INTERRUPT_RUNTIME
#define RUNE_ARDUINO_ENABLE_INTERRUPT_RUNTIME 1
#endif

#ifndef RUNE_ARDUINO_ENABLE_RANDOM_RUNTIME
#define RUNE_ARDUINO_ENABLE_RANDOM_RUNTIME 1
#endif

#ifndef RUNE_ARDUINO_ENABLE_GPIO_RUNTIME
#define RUNE_ARDUINO_ENABLE_GPIO_RUNTIME 1
#endif

#ifndef RUNE_ARDUINO_ENABLE_SHIFT_RUNTIME
#define RUNE_ARDUINO_ENABLE_SHIFT_RUNTIME 1
#endif

#ifndef RUNE_ARDUINO_ENABLE_TONE_RUNTIME
#define RUNE_ARDUINO_ENABLE_TONE_RUNTIME 1
#endif

#ifndef RUNE_ARDUINO_ENABLE_UART_PEEK_RUNTIME
#define RUNE_ARDUINO_ENABLE_UART_PEEK_RUNTIME 1
#endif

#ifndef RUNE_AVR_TARGET_TRIPLE
#define RUNE_AVR_TARGET_TRIPLE "avr-atmega328p-arduino-uno"
#endif

#ifndef RUNE_AVR_BOARD_NAME
#define RUNE_AVR_BOARD_NAME "arduino-uno"
#endif

static char rune_input_buffer[RUNE_INPUT_BUFFER_SIZE];
static uint16_t rune_last_string_len = 0;
static bool rune_serial_is_open_flag = false;

#if RUNE_ARDUINO_ENABLE_STRING_RUNTIME
static char rune_string_slots[RUNE_STRING_SLOT_COUNT][RUNE_STRING_SLOT_SIZE];
static uint8_t rune_string_slot_index = 0;
#endif
#ifdef RUNE_ARDUINO_ENABLE_SERVO
alignas(Servo) static unsigned char rune_servo_storage[20][sizeof(Servo)];
static Servo* rune_servo_slots[20] = { nullptr };
static bool rune_servo_constructed_flags[20] = { false };
static bool rune_servo_attached_flags[20] = { false };
#endif

#if RUNE_ARDUINO_ENABLE_STRING_RUNTIME
static char* rune_claim_string_slot(void) {
    char* slot = rune_string_slots[rune_string_slot_index];
    rune_string_slot_index = (uint8_t)((rune_string_slot_index + 1) % RUNE_STRING_SLOT_COUNT);
    slot[0] = '\0';
    return slot;
}

static void* rune_store_temp_string(const char* text) {
    char* slot = rune_claim_string_slot();
    size_t len = strlen(text);
    if (len >= RUNE_STRING_SLOT_SIZE) {
        len = RUNE_STRING_SLOT_SIZE - 1;
    }
    memcpy(slot, text, len);
    slot[len] = '\0';
    rune_last_string_len = (int64_t)len;
    return slot;
}

static void* rune_store_temp_bytes(const void* ptr, size_t len) {
    char* slot = rune_claim_string_slot();
    if (len >= RUNE_STRING_SLOT_SIZE) {
        len = RUNE_STRING_SLOT_SIZE - 1;
    }
    memcpy(slot, ptr, len);
    slot[len] = '\0';
    rune_last_string_len = (int64_t)len;
    return slot;
}

static const char* rune_store_string_literal(const char* text) {
    rune_last_string_len = (int64_t)strlen(text);
    return text;
}
#endif

static int rune_compare_bytes(const uint8_t* left, uint64_t left_len, const uint8_t* right, uint64_t right_len) {
    size_t left_size = left_len > (uint64_t)SIZE_MAX ? SIZE_MAX : (size_t)left_len;
    size_t right_size = right_len > (uint64_t)SIZE_MAX ? SIZE_MAX : (size_t)right_len;
    size_t shared = left_size < right_size ? left_size : right_size;
    if (shared > 0) {
        int cmp = memcmp(left, right, shared);
        if (cmp < 0) {
            return -1;
        }
        if (cmp > 0) {
            return 1;
        }
    }
    if (left_len < right_len) {
        return -1;
    }
    if (left_len > right_len) {
        return 1;
    }
    return 0;
}

extern "C" int64_t rune_rt_last_string_len(void) {
    return (int64_t)rune_last_string_len;
}

extern "C" void rune_rt_print_str(const char* text, uint64_t len) {
#if RUNE_ARDUINO_ENABLE_SERIAL_WRAPPER_RUNTIME
    Serial.write((const uint8_t*)text, (size_t)len);
#endif
}

extern "C" void rune_rt_eprint_str(const char* text, uint64_t len) {
    rune_rt_print_str(text, len);
}

extern "C" void rune_rt_print_i64(int64_t value) {
#if RUNE_ARDUINO_ENABLE_SERIAL_WRAPPER_RUNTIME
    char buffer[24];
    uint8_t index = 0;
    uint64_t magnitude = (value < 0) ? (uint64_t)(-value) : (uint64_t)value;
    if (value == 0) {
        Serial.write('0');
        return;
    }
    if (value < 0) {
        Serial.write('-');
    }
    while (magnitude > 0) {
        buffer[index++] = (char)('0' + (magnitude % 10));
        magnitude /= 10;
    }
    while (index > 0) {
        Serial.write(buffer[--index]);
    }
#endif
}

extern "C" void rune_rt_eprint_i64(int64_t value) {
    rune_rt_print_i64(value);
}

extern "C" void rune_rt_print_bool(int64_t value) {
#if RUNE_ARDUINO_ENABLE_SERIAL_WRAPPER_RUNTIME
    Serial.print(value != 0 ? "true" : "false");
#endif
}

extern "C" void rune_rt_eprint_bool(int64_t value) {
    rune_rt_print_bool(value);
}

extern "C" void rune_rt_print_newline(void) {
#if RUNE_ARDUINO_ENABLE_SERIAL_WRAPPER_RUNTIME
    Serial.write('\r');
    Serial.write('\n');
#endif
}

extern "C" void rune_rt_fail(int32_t code) {
#if RUNE_ARDUINO_ENABLE_SERIAL_WRAPPER_RUNTIME || defined(RUNE_ARDUINO_FORCE_SERIAL_FAIL)
    if (!rune_serial_is_open_flag) {
        Serial.begin(115200);
        rune_serial_is_open_flag = true;
    }
    rune_rt_print_str("ERR E", 5);
    rune_rt_print_i64((int64_t)code);
    rune_rt_print_newline();
#endif
    for (;;) {
        delay(1000);
    }
}

static int64_t rune_checked_div_i64(int64_t left, int64_t right) {
    if (right == 0) {
        rune_rt_fail(1001);
        return 0;
    }
    return left / right;
}

static int64_t rune_checked_mod_i64(int64_t left, int64_t right) {
    if (right == 0) {
        rune_rt_fail(1002);
        return 0;
    }
    return left % right;
}

extern "C" void rune_rt_eprint_newline(void) {
    rune_rt_print_newline();
}

extern "C" int32_t rune_rt_string_compare(const char* left_ptr, uint64_t left_len, const char* right_ptr, uint64_t right_len) {
    return rune_compare_bytes((const uint8_t*)left_ptr, left_len, (const uint8_t*)right_ptr, right_len);
}

extern "C" bool rune_rt_string_equal(const char* left_ptr, uint64_t left_len, const char* right_ptr, uint64_t right_len) {
    return left_len == right_len
        && memcmp(left_ptr, right_ptr, (size_t)(left_len > (uint64_t)SIZE_MAX ? SIZE_MAX : left_len)) == 0;
}

#if RUNE_ARDUINO_ENABLE_STRING_RUNTIME
extern "C" void* rune_rt_string_from_i64(int64_t value) {
    char* slot = rune_claim_string_slot();
    uint8_t index = 0;
    uint64_t magnitude = (value < 0) ? (uint64_t)(-value) : (uint64_t)value;
    if (value == 0) {
        slot[0] = '0';
        slot[1] = '\0';
        rune_last_string_len = 1;
        return slot;
    }
    if (value < 0) {
        slot[index++] = '-';
    }
    char reversed[21];
    uint8_t digits = 0;
    while (magnitude > 0 && digits + 1 < sizeof(reversed)) {
        reversed[digits++] = (char)('0' + (magnitude % 10));
        magnitude /= 10;
    }
    while (digits > 0 && index + 1 < RUNE_STRING_SLOT_SIZE) {
        slot[index++] = reversed[--digits];
    }
    slot[index] = '\0';
    rune_last_string_len = (int64_t)index;
    return slot;
}

extern "C" void* rune_rt_string_from_bool(bool value) {
    return (void*)rune_store_string_literal(value ? "true" : "false");
}

extern "C" void* rune_rt_string_concat(const char* left_ptr, int64_t left_len, const char* right_ptr, int64_t right_len) {
    char* slot = rune_claim_string_slot();
    size_t used = 0;
    size_t left_copy = left_len < (int64_t)(RUNE_STRING_SLOT_SIZE - 1) ? (size_t)left_len : RUNE_STRING_SLOT_SIZE - 1;
    memcpy(slot, left_ptr, left_copy);
    used += left_copy;
    size_t remaining = RUNE_STRING_SLOT_SIZE - used - 1;
    size_t right_copy = right_len < (int64_t)remaining ? (size_t)right_len : remaining;
    memcpy(slot + used, right_ptr, right_copy);
    used += right_copy;
    slot[used] = '\0';
    rune_last_string_len = (int64_t)used;
    return slot;
}
#endif

extern "C" int64_t rune_rt_string_to_i64(const char* ptr, uint64_t len) {
    char buffer[32];
    size_t copy_len = len < sizeof(buffer) - 1 ? (size_t)len : sizeof(buffer) - 1;
    memcpy(buffer, ptr, copy_len);
    buffer[copy_len] = '\0';
    const char* text = buffer;
    bool negative = false;
    if (*text == '-') {
        negative = true;
        ++text;
    }
    int64_t value = 0;
    while (*text >= '0' && *text <= '9') {
        value = (value * 10) + (int64_t)(*text - '0');
        ++text;
    }
    return negative ? -value : value;
}



#if RUNE_ARDUINO_ENABLE_DYNAMIC_RUNTIME
extern "C" void* rune_rt_dynamic_to_string(int64_t tag, int64_t payload, int64_t extra) {
    switch (tag) {
        case 0:
            return (void*)rune_store_string_literal("unit");
        case 1:
            return (void*)rune_store_string_literal(payload != 0 ? "true" : "false");
        case 2: {
            return rune_rt_string_from_i64((int32_t)payload);
        }
        case 3: {
            return rune_rt_string_from_i64(payload);
        }
        case 4:
            return rune_store_temp_bytes((const void*)(uintptr_t)payload, (size_t)extra);
        default:
            return (void*)rune_store_string_literal("<dynamic>");
    }
}

extern "C" void rune_rt_print_dynamic(int64_t tag, int64_t payload, int64_t extra) {
    const char* text = (const char*)rune_rt_dynamic_to_string(tag, payload, extra);
    rune_rt_print_str(text, (uint64_t)rune_last_string_len);
}

extern "C" void rune_rt_eprint_dynamic(int64_t tag, int64_t payload, int64_t extra) {
    const char* text = (const char*)rune_rt_dynamic_to_string(tag, payload, extra);
    rune_rt_eprint_str(text, (uint64_t)rune_last_string_len);
}
#endif

#if RUNE_ARDUINO_ENABLE_SYSTEM_RUNTIME
extern "C" bool rune_rt_system_is_embedded(void) {
    return true;
}

extern "C" bool rune_rt_system_is_wasm(void) {
    return false;
}

#if RUNE_ARDUINO_ENABLE_STRING_RUNTIME
extern "C" void* rune_rt_system_platform(void) {
    return (void*)rune_store_string_literal("embedded");
}

extern "C" void* rune_rt_system_arch(void) {
    return (void*)rune_store_string_literal("avr");
}

extern "C" void* rune_rt_system_target(void) {
    return (void*)rune_store_string_literal(RUNE_AVR_TARGET_TRIPLE);
}

extern "C" void* rune_rt_system_board(void) {
    return (void*)rune_store_string_literal(RUNE_AVR_BOARD_NAME);
}
#endif
#endif

#if RUNE_ARDUINO_ENABLE_ENV_RUNTIME
extern "C" int32_t rune_rt_env_arg_count(void) {
    return 0;
}

extern "C" void* rune_rt_env_get_string(void* ptr, int64_t len, void* default_ptr, int64_t default_len) {
    (void)ptr;
    (void)len;
    rune_last_string_len = default_len;
    return default_ptr;
}

extern "C" void* rune_rt_env_arg(int32_t index) {
    (void)index;
    return (void*)rune_store_string_literal("");
}
#endif

extern "C" int64_t rune_rt_sum_range(int64_t start, int64_t stop, int64_t step) {
    if (step == 0) {
        return 0;
    }
    int64_t total = 0;
    if (step > 0) {
        for (int64_t value = start; value < stop; value += step) {
            total += value;
        }
    } else {
        for (int64_t value = start; value > stop; value += step) {
            total += value;
        }
    }
    return total;
}

#if RUNE_ARDUINO_ENABLE_DYNAMIC_RUNTIME
extern "C" void rune_rt_dynamic_binary(const int64_t* left, const int64_t* right, int64_t* out, int64_t op) {
    int64_t left_tag = left[0];
    int64_t left_payload = left[1];
    int64_t left_extra = left[2];
    int64_t right_tag = right[0];
    int64_t right_payload = right[1];
    int64_t right_extra = right[2];

    if (op == 0 && (left_tag == 4 || right_tag == 4)) {
        void* left_text = rune_rt_dynamic_to_string(left_tag, left_payload, left_extra);
        int64_t left_len = rune_rt_last_string_len();
        void* right_text = rune_rt_dynamic_to_string(right_tag, right_payload, right_extra);
        int64_t right_len = rune_rt_last_string_len();
        char* concat_buffer = rune_claim_string_slot();
        size_t used = 0;
        size_t left_copy = left_len < (int64_t)(RUNE_STRING_SLOT_SIZE - 1) ? (size_t)left_len : RUNE_STRING_SLOT_SIZE - 1;
        memcpy(concat_buffer, left_text, left_copy);
        used += left_copy;
        size_t remaining = RUNE_STRING_SLOT_SIZE - used - 1;
        size_t right_copy = right_len < (int64_t)remaining ? (size_t)right_len : remaining;
        memcpy(concat_buffer + used, right_text, right_copy);
        used += right_copy;
        concat_buffer[used] = '\0';
        out[0] = 4;
        out[1] = (int64_t)(intptr_t)concat_buffer;
        out[2] = (int64_t)used;
        rune_last_string_len = (int64_t)used;
        return;
    }

    int64_t left_value = (left_tag == 4) ? rune_rt_string_to_i64((const char*)(intptr_t)left_payload, (uint64_t)left_extra) :
                        (left_tag == 1 ? (left_payload != 0 ? 1 : 0) :
                        (left_tag == 2 ? (int32_t)left_payload : left_payload));
    int64_t right_value = (right_tag == 4) ? rune_rt_string_to_i64((const char*)(intptr_t)right_payload, (uint64_t)right_extra) :
                         (right_tag == 1 ? (right_payload != 0 ? 1 : 0) :
                         (right_tag == 2 ? (int32_t)right_payload : right_payload));
    out[0] = 3;
    out[2] = 0;
    switch (op) {
        case 0: out[1] = left_value + right_value; break;
        case 1: out[1] = left_value - right_value; break;
        case 2: out[1] = left_value * right_value; break;
        case 3: out[1] = rune_checked_div_i64(left_value, right_value); break;
        case 4: out[1] = rune_checked_mod_i64(left_value, right_value); break;
        default: out[1] = 0; break;
    }
}

extern "C" bool rune_rt_dynamic_compare(const int64_t* left, const int64_t* right, int64_t op) {
    int64_t left_tag = left[0];
    int64_t left_payload = left[1];
    int64_t left_extra = left[2];
    int64_t right_tag = right[0];
    int64_t right_payload = right[1];
    int64_t right_extra = right[2];

    if (left_tag == 4 || right_tag == 4) {
        const char* left_text = (const char*)rune_rt_dynamic_to_string(left_tag, left_payload, left_extra);
        uint64_t left_len = (uint64_t)rune_rt_last_string_len();
        const char* right_text = (const char*)rune_rt_dynamic_to_string(right_tag, right_payload, right_extra);
        uint64_t right_len = (uint64_t)rune_rt_last_string_len();
        int cmp = rune_rt_string_compare(left_text, left_len, right_text, right_len);
        switch (op) {
            case 0: return cmp == 0;
            case 1: return cmp != 0;
            case 2: return cmp > 0;
            case 3: return cmp >= 0;
            case 4: return cmp < 0;
            case 5: return cmp <= 0;
            default: return false;
        }
    }

    int64_t left_value = (left_tag == 1 ? (left_payload != 0 ? 1 : 0) :
                         (left_tag == 2 ? (int32_t)left_payload : left_payload));
    int64_t right_value = (right_tag == 1 ? (right_payload != 0 ? 1 : 0) :
                          (right_tag == 2 ? (int32_t)right_payload : right_payload));
    switch (op) {
        case 0: return left_value == right_value;
        case 1: return left_value != right_value;
        case 2: return left_value > right_value;
        case 3: return left_value >= right_value;
        case 4: return left_value < right_value;
        case 5: return left_value <= right_value;
        default: return false;
    }
}
#endif

extern "C" void rune_rt_arduino_uart_begin(int64_t baud) {
    Serial.begin((unsigned long)baud);
    rune_serial_is_open_flag = true;
}

extern "C" int64_t rune_rt_arduino_uart_available(void) {
    return (int64_t)Serial.available();
}

extern "C" int64_t rune_rt_arduino_uart_read_byte(void) {
    return (int64_t)Serial.read();
}

#if RUNE_ARDUINO_ENABLE_UART_PEEK_RUNTIME
extern "C" int64_t rune_rt_arduino_uart_peek_byte(void) {
    return (int64_t)Serial.peek();
}
#endif

extern "C" void rune_rt_arduino_uart_write_byte(int64_t value) {
    Serial.write((uint8_t)value);
}

extern "C" void rune_rt_arduino_uart_write(void* text, uint64_t len) {
    Serial.write((const uint8_t*)text, (size_t)len);
}

#if RUNE_ARDUINO_ENABLE_INTERRUPT_RUNTIME
extern "C" void rune_rt_arduino_interrupts_enable(void) {
    interrupts();
}

extern "C" void rune_rt_arduino_interrupts_disable(void) {
    noInterrupts();
}
#endif

#if RUNE_ARDUINO_ENABLE_RANDOM_RUNTIME
extern "C" void rune_rt_arduino_random_seed(int64_t seed) {
    randomSeed((unsigned long)seed);
}

extern "C" int64_t rune_rt_arduino_random_i64(int64_t max_value) {
    if (max_value <= 0) {
        return 0;
    }
    return (int64_t)random((long)max_value);
}

extern "C" int64_t rune_rt_arduino_random_range(int64_t min_value, int64_t max_value) {
    if (max_value <= min_value) {
        return min_value;
    }
    return (int64_t)random((long)min_value, (long)max_value);
}
#endif

static void* rune_read_serial_line(void) {
    uint8_t index = 0;
    for (;;) {
        int value = Serial.read();
        if (value < 0) {
            continue;
        }
        if (value == '\r') {
            continue;
        }
        if (value == '\n') {
            break;
        }
        if (index + 1 < sizeof(rune_input_buffer)) {
            rune_input_buffer[index++] = (char)value;
        }
    }
    rune_input_buffer[index] = '\0';
    rune_last_string_len = (int64_t)index;
    return rune_input_buffer;
}

extern "C" void* rune_rt_arduino_read_line(void) {
    return rune_read_serial_line();
}

extern "C" void* rune_rt_input_line(void) {
    return rune_read_serial_line();
}

#if RUNE_ARDUINO_ENABLE_SERIAL_WRAPPER_RUNTIME
extern "C" bool rune_rt_serial_is_open(void) {
    return rune_serial_is_open_flag;
}

extern "C" bool rune_rt_serial_open(void* port_ptr, uint64_t port_len, uint64_t baud) {
    (void)port_ptr;
    (void)port_len;
    Serial.begin((unsigned long)baud);
    rune_serial_is_open_flag = true;
    return true;
}

extern "C" bool rune_rt_serial_write(void* text, uint64_t len) {
    Serial.write((const uint8_t*)text, (size_t)len);
    return true;
}

extern "C" bool rune_rt_serial_write_line(void* text, uint64_t len) {
    Serial.write((const uint8_t*)text, (size_t)len);
    rune_rt_print_newline();
    return true;
}

extern "C" int64_t rune_rt_serial_peek_byte(void) {
    return (int64_t)Serial.peek();
}

extern "C" bool rune_rt_serial_write_byte(int64_t value) {
    Serial.write((uint8_t)value);
    return true;
}

extern "C" void rune_rt_serial_flush(void) {
    Serial.flush();
}

extern "C" void* rune_rt_serial_read_line(void) {
    return rune_read_serial_line();
}

extern "C" void rune_rt_serial_close(void) {
    Serial.flush();
}

// Serial module bridge: non-embedded API names that are emitted even on AVR
// due to the system_is_embedded() branches not being constant-folded by LLVM.
extern "C" int64_t rune_rt_serial_available(void) {
    return (int64_t)Serial.available();
}

extern "C" int64_t rune_rt_serial_read_byte(void) {
    while (Serial.available() <= 0) {}
    return (int64_t)Serial.read();
}

extern "C" int64_t rune_rt_serial_read_byte_timeout(int64_t timeout_ms) {
    int64_t deadline = (int64_t)millis() + timeout_ms;
    while ((int64_t)millis() < deadline) {
        if (Serial.available() > 0) {
            return (int64_t)Serial.read();
        }
    }
    return -1;
}
#endif

#if RUNE_ARDUINO_ENABLE_GPIO_RUNTIME
extern "C" void rune_rt_arduino_pin_mode(int64_t pin, int64_t mode) {
    pinMode((uint8_t)pin, (uint8_t)mode);
}

extern "C" void rune_rt_arduino_digital_write(int64_t pin, bool value) {
    digitalWrite((uint8_t)pin, value ? HIGH : LOW);
}

extern "C" bool rune_rt_arduino_digital_read(int64_t pin) {
    return digitalRead((uint8_t)pin) == HIGH;
}

extern "C" void rune_rt_arduino_analog_write(int64_t pin, int64_t value) {
    analogWrite((uint8_t)pin, (int)value);
}

extern "C" int64_t rune_rt_arduino_analog_read(int64_t pin) {
    return analogRead((uint8_t)pin);
}

extern "C" void rune_rt_arduino_analog_reference(int64_t mode) {
    analogReference((uint8_t)mode);
}

extern "C" int64_t rune_rt_arduino_pulse_in(int64_t pin, bool state, int64_t timeout_us) {
    return (int64_t)pulseIn((uint8_t)pin, state ? HIGH : LOW, (unsigned long)timeout_us);
}

#ifdef RUNE_ARDUINO_ENABLE_SERVO
extern "C" bool rune_rt_arduino_servo_attach(int64_t pin) {
    if (pin < 0 || pin >= 20) {
        return false;
    }
    uint8_t slot = (uint8_t)pin;
    if (rune_servo_slots[slot] == nullptr) {
        rune_servo_slots[slot] = new (&rune_servo_storage[slot][0]) Servo();
        rune_servo_constructed_flags[slot] = true;
    }
    if (!rune_servo_attached_flags[slot]) {
        rune_servo_slots[slot]->attach((int)pin);
        rune_servo_attached_flags[slot] = true;
    }
    return rune_servo_attached_flags[slot];
}

extern "C" void rune_rt_arduino_servo_detach(int64_t pin) {
    if (pin < 0 || pin >= 20) {
        return;
    }
    uint8_t slot = (uint8_t)pin;
    if (rune_servo_slots[slot] != nullptr && rune_servo_attached_flags[slot]) {
        rune_servo_slots[slot]->detach();
        rune_servo_attached_flags[slot] = false;
    }
}

extern "C" void rune_rt_arduino_servo_write(int64_t pin, int64_t angle) {
    if (!rune_rt_arduino_servo_attach(pin)) {
        return;
    }
    if (angle < 0) {
        angle = 0;
    } else if (angle > 180) {
        angle = 180;
    }
    rune_servo_slots[(uint8_t)pin]->write((int)angle);
}

extern "C" void rune_rt_arduino_servo_write_us(int64_t pin, int64_t pulse_us) {
    if (!rune_rt_arduino_servo_attach(pin)) {
        return;
    }
    rune_servo_slots[(uint8_t)pin]->writeMicroseconds((int)pulse_us);
}
#endif

extern "C" void rune_rt_arduino_delay_ms(int64_t ms) {
    delay((unsigned long)ms);
}

extern "C" void rune_rt_arduino_delay_us(int64_t us) {
    delayMicroseconds((unsigned int)us);
}

extern "C" int64_t rune_rt_time_now_unix(void) {
    rune_rt_fail(1100);
    return 0;
}

extern "C" bool rune_rt_time_has_wall_clock(void) {
    return false;
}

extern "C" int64_t rune_rt_time_monotonic_ms(void) {
    return (int64_t)millis();
}

extern "C" int64_t rune_rt_time_monotonic_us(void) {
    return (int64_t)micros();
}

extern "C" void rune_rt_time_sleep_ms(int64_t ms) {
    delay((unsigned long)ms);
}

extern "C" void rune_rt_time_sleep_us(int64_t us) {
    delayMicroseconds((unsigned int)us);
}

extern "C" int64_t rune_rt_arduino_millis(void) {
    return (int64_t)millis();
}

extern "C" int64_t rune_rt_arduino_micros(void) {
    return (int64_t)micros();
}

extern "C" int64_t rune_rt_arduino_mode_input(void) {
    return INPUT;
}

extern "C" int64_t rune_rt_arduino_mode_output(void) {
    return OUTPUT;
}

extern "C" int64_t rune_rt_arduino_mode_input_pullup(void) {
    return INPUT_PULLUP;
}

extern "C" int64_t rune_rt_arduino_led_builtin(void) {
    return LED_BUILTIN;
}

extern "C" int64_t rune_rt_arduino_high(void) {
    return HIGH;
}

extern "C" int64_t rune_rt_arduino_low(void) {
    return LOW;
}

extern "C" int64_t rune_rt_arduino_analog_ref_default(void) {
    return DEFAULT;
}

extern "C" int64_t rune_rt_arduino_analog_ref_internal(void) {
    return INTERNAL;
}

extern "C" int64_t rune_rt_arduino_analog_ref_external(void) {
    return EXTERNAL;
}

// GPIO module bridge functions (used by the builtin gpio/pwm/adc modules)
extern "C" void rune_rt_gpio_pin_mode(int64_t pin, int64_t mode) {
    rune_rt_arduino_pin_mode(pin, mode);
}

extern "C" void rune_rt_gpio_digital_write(int64_t pin, bool value) {
    rune_rt_arduino_digital_write(pin, value);
}

extern "C" bool rune_rt_gpio_digital_read(int64_t pin) {
    return rune_rt_arduino_digital_read(pin);
}

extern "C" void rune_rt_gpio_pwm_write(int64_t pin, int64_t value) {
    rune_rt_arduino_analog_write(pin, value);
}

extern "C" int64_t rune_rt_gpio_analog_read(int64_t pin) {
    return rune_rt_arduino_analog_read(pin);
}

extern "C" int64_t rune_rt_gpio_mode_input(void) { return INPUT; }
extern "C" int64_t rune_rt_gpio_mode_output(void) { return OUTPUT; }
extern "C" int64_t rune_rt_gpio_mode_input_pullup(void) { return INPUT_PULLUP; }
extern "C" int64_t rune_rt_gpio_pwm_duty_max(void) { return 255; }
extern "C" int64_t rune_rt_gpio_analog_max(void) { return 1023; }
#endif

// Bit-order constants — needed by both GPIO and shift operations
extern "C" int64_t rune_rt_arduino_bit_order_lsb_first(void) {
    return LSBFIRST;
}

extern "C" int64_t rune_rt_arduino_bit_order_msb_first(void) {
    return MSBFIRST;
}

#if !RUNE_ARDUINO_ENABLE_GPIO_RUNTIME
extern "C" void rune_rt_arduino_delay_ms(int64_t ms) {
    delay((unsigned long)ms);
}

extern "C" void rune_rt_arduino_delay_us(int64_t us) {
    delayMicroseconds((unsigned int)us);
}

extern "C" int64_t rune_rt_time_now_unix(void) {
    rune_rt_fail(1100);
    return 0;
}

extern "C" bool rune_rt_time_has_wall_clock(void) {
    return false;
}

extern "C" int64_t rune_rt_time_monotonic_ms(void) {
    return (int64_t)millis();
}

extern "C" int64_t rune_rt_time_monotonic_us(void) {
    return (int64_t)micros();
}

extern "C" void rune_rt_time_sleep_ms(int64_t ms) {
    delay((unsigned long)ms);
}

extern "C" void rune_rt_time_sleep_us(int64_t us) {
    delayMicroseconds((unsigned int)us);
}

extern "C" int64_t rune_rt_arduino_millis(void) {
    return (int64_t)millis();
}

extern "C" int64_t rune_rt_arduino_micros(void) {
    return (int64_t)micros();
}
#endif

#if RUNE_ARDUINO_ENABLE_SHIFT_RUNTIME
extern "C" void rune_rt_arduino_shift_out(int64_t data_pin, int64_t clock_pin, int64_t bit_order, int64_t value) {
    shiftOut((uint8_t)data_pin, (uint8_t)clock_pin, (uint8_t)bit_order, (uint8_t)value);
}

extern "C" int64_t rune_rt_arduino_shift_in(int64_t data_pin, int64_t clock_pin, int64_t bit_order) {
    return (int64_t)shiftIn((uint8_t)data_pin, (uint8_t)clock_pin, (uint8_t)bit_order);
}
#endif

#if RUNE_ARDUINO_ENABLE_TONE_RUNTIME
extern "C" void rune_rt_arduino_tone(int64_t pin, int64_t frequency_hz, int64_t duration_ms) {
    tone((uint8_t)pin, (unsigned int)frequency_hz, (unsigned long)duration_ms);
}

extern "C" void rune_rt_arduino_no_tone(int64_t pin) {
    noTone((uint8_t)pin);
}
#endif

#if defined(RUNE_ARDUINO_ENTRY_MAIN)
extern "C" int rune_entry_main(void);
static bool rune_main_executed = false;

void setup(void) {
    if (!rune_main_executed) {
        rune_main_executed = true;
        rune_entry_main();
    }
}

void loop(void) {}
#elif defined(RUNE_ARDUINO_ENTRY_SETUP_LOOP)
extern "C" void rune_entry_setup(void);
extern "C" void rune_entry_loop(void);

void setup(void) { rune_entry_setup(); }
void loop(void) { rune_entry_loop(); }
#else
#error "RUNE_ARDUINO_ENTRY_MAIN or RUNE_ARDUINO_ENTRY_SETUP_LOOP must be defined"
#endif
