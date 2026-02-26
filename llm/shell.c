#include "shell.h"
#include "../kernel/include/vga.h"
#include "../kernel/include/fs.h"
#include <string.h>

static llm_context_t ctx;
static const char *system_prompt = 
    "You are the shell of LLMOS. Interpret user commands and output JSON actions.\n"
    "\n"
    "Available actions:\n"
    "- create_file(path, content)\n"
    "- read_file(path)\n"
    "- write_file(path, content)\n"
    "- delete_file(path)\n"
    "- list_dir()\n"
    "- clear_screen()\n"
    "- exit()\n"
    "\n"
    "Always respond with a JSON object containing:\n"
    "{\"action\": \"action_name\", \"params\": {...}}\n"
    "\n"
    "Example:\n"
    "User: \"open a text document and type hello world\"\n"
    "Response: {\"action\": \"create_file\", \"params\": {\"path\": \"document.txt\", \"content\": \"hello world\"}}\n";

int llm_init(void) {
    ctx.model_loaded = 0;
    vga_write("LLM Shell initialized.\n");
    vga_write("Note: LLM model not loaded. Using command parser mode.\n");
    return 0;
}

void llm_shutdown(void) {
    if (ctx.model_loaded) {
    }
}

static void parse_and_execute(const char *json_action) {
    if (strstr(json_action, "create_file") != 0) {
        char *path_start = strstr(json_action, "\"path\"");
        char *content_start = strstr(json_action, "\"content\"");
        if (path_start && content_start) {
            char filename[64] = {0};
            char content[256] = {0};
            
            path_start = strchr(path_start, ':');
            if (path_start) path_start++;
            while (*path_start && (*path_start == ' ' || *path_start == '"' || *path_start == ':')) path_start++;
            int i = 0;
            while (*path_start && *path_start != '"' && i < 63) filename[i++] = *path_start++;
            filename[i] = 0;
            
            content_start = strchr(content_start, ':');
            if (content_start) content_start++;
            while (*content_start && (*content_start == ' ' || *content_start == '"' || *content_start == ':')) content_start++;
            i = 0;
            while (*content_start && *content_start != '"' && i < 255) content[i++] = *content_start++;
            content[i] = 0;
            
            file_t *f = fs_create(filename);
            if (f) {
                fs_write(f, content, strlen(content));
                vga_write("File created: ");
                vga_write(filename);
                vga_write("\n");
            }
        }
    } else if (strstr(json_action, "delete_file") != 0) {
        char *path_start = strstr(json_action, "\"path\"");
        if (path_start) {
            char filename[64] = {0};
            path_start = strchr(path_start, ':');
            if (path_start) path_start++;
            while (*path_start && (*path_start == ' ' || *path_start == '"' || *path_start == ':')) path_start++;
            int i = 0;
            while (*path_start && *path_start != '"' && i < 63) filename[i++] = *path_start++;
            filename[i] = 0;
            
            if (fs_delete(filename)) {
                vga_write("File deleted: ");
                vga_write(filename);
                vga_write("\n");
            }
        }
    } else if (strstr(json_action, "list_dir") != 0) {
        fs_list();
    } else if (strstr(json_action, "clear_screen") != 0) {
        vga_clear();
    } else {
        vga_write("Action not recognized. Use help for commands.\n");
    }
}

const char *llm_process(const char *input) {
    if (strlen(input) == 0) {
        return "{\"action\": \"none\"}";
    }
    
    if (strncmp(input, "help", 4) == 0) {
        return "{\"action\": \"help\"}";
    }
    else if (strncmp(input, "ls", 2) == 0) {
        return "{\"action\": \"list_dir\"}";
    }
    else if (strncmp(input, "clear", 5) == 0) {
        return "{\"action\": \"clear_screen\"}";
    }
    else if (strncmp(input, "exit", 4) == 0) {
        return "{\"action\": \"exit\"}";
    }
    else if (strstr(input, "create") != 0 || strstr(input, "file") != 0) {
        return "{\"action\": \"create_file\"}";
    }
    else if (strstr(input, "delete") != 0 || strstr(input, "remove") != 0) {
        return "{\"action\": \"delete_file\"}";
    }
    
    return "{\"action\": \"unknown\"}";
}
