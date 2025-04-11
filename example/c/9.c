int main() {
    int a = 1, b = 2, c = 3;
    if (a != 1)
        if (b != 2)
            return 1;
        else {
            int d = 1;
            return 0;
        }
    return 3;
}