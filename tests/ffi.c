#include <stdio.h>

extern int sloth_lang_run_string(char *prog);
char *code = "print(\"hello world.    \");\n";

int main() {
    int ret = sloth_lang_run_string(code);
    fflush(stdout);
    printf("ret: %d", ret);
    return 0;
}