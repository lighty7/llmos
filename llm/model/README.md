# LLM Model Setup for llmos

This directory is for storing the LLM model files.

## Recommended Models

### TinyLlama-1.1B (Recommended for Start)
- **Parameters**: 1.1 billion
- **Size (Q4_K_M)**: ~700MB
- **RAM Required**: ~1GB
- **Source**: https://huggingface.co/TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF
- **File**: `tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf`

### Phi-3 Mini (Better Performance)
- **Parameters**: 3.8 billion
- **Size (Q4_K_M)**: ~2GB
- **RAM Required**: ~4GB
- **Source**: https://huggingface.co/TheBloke/Phi-3-mini-4k-instruct-GGUF
- **File**: `phi-3-mini-4k-instruct.Q4_K_M.gguf`

### Qwen2-0.5B (Smallest)
- **Parameters**: 500 million
- **Size (Q4)**: ~400MB
- **RAM Required**: ~700MB
- **Source**: https://huggingface.co/Qwen/Qwen2-0.5B-Instruct-GGUF

## Installation

### Option 1: Download Pre-quantized GGUF

```bash
cd llm/model

# Download TinyLlama (recommended for start)
wget https://huggingface.co/TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF/resolve/main/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf

# Or download Phi-3 Mini (better performance, needs more RAM)
wget https://huggingface.co/TheBloke/Phi-3-mini-4k-instruct-GGUF/resolve/main/phi-3-mini-4k-instruct.Q4_K_M.gguf
```

### Option 2: Build from Source

```bash
# Clone llama.cpp
git clone https://github.com/ggerganov/llama.cpp.git

# Build
cd llama.cpp
make

# Download model and convert
# (requires more setup - see llama.cpp docs)
```

## Integration

Once you have the model file:

1. Copy the `.gguf` file to this directory
2. Edit `llm/shell.c` to enable full LLM inference
3. Rebuild the kernel

## Full LLM Integration (Future)

Currently, llmos uses a command parser. To integrate the full LLM:

1. Add llama.cpp as a submodule:
   ```bash
   git submodule add https://github.com/ggerganov/llama.cpp.git llm/llama.cpp
   ```

2. Build llama.cpp static library for your OS

3. Modify `llm/shell.c` to:
   - Load the model on boot
   - Use `llama_decode()` for inference
   - Parse the output for JSON actions

## System Prompt

The LLM is configured with this system prompt:

```
You are the shell of LLMOS. Interpret user commands and output JSON actions.

Available actions:
- create_file(path, content)
- read_file(path)
- write_file(path, content)
- delete_file(path)
- list_dir()
- clear_screen()
- exit()

Always respond with JSON with fields: action, params
```

## Example Interactions

Once fully integrated:

```
llmos> open a text document and type hello world
{"action": "create_file", "params": {"path": "document.txt", "content": "hello world"}}

llmos> list my files
{"action": "list_dir"}

llmos> create a folder called projects
{"action": "create_dir", "params": {"path": "projects"}}
```

## Notes

- Start with TinyLlama - it runs on most systems
- Full LLM integration requires significant RAM (2GB+)
- For production, consider model quantization to reduce size
