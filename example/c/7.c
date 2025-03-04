int main() {
    const int M = 10007;
    // int x = 2;
    // int y = x * x % M;
    int x;
    x = 2;
    int y;
    y = x * x % M;
    y = x * y % M;
    return y;
}