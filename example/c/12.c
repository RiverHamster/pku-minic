int main() {
    int a = 1 < 2;
    int b = 3;
    int c = a && b;
    int d = (a && b) + (!c && b);
    return d;
}