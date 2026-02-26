#ifndef LLM_SHELL_H
#define LLM_SHELL_H

#include <stdint.h>

#define LLM_MAX_INPUT 512
#define LLM_MAX_OUTPUT 1024

typedef struct {
    char input[LLM_MAX_INPUT];
    char output[LLM_MAX_OUTPUT];
    int model_loaded;
} llm_context_t;

int llm_init(void);
void llm_shutdown(void);
const char *llm_process(const char *input);

#endif
