const char* greet_from_c(const char* name) {
    return (name[0] == 'R' && name[1] == 'u' && name[2] == 'n' && name[3] == 'e' && name[4] == '\0')
        ? "hi from c"
        : "unknown";
}
