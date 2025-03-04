int main() {
    const int x = 3;
    const int y = x * x + x;
    const int z = (y + 1) % x;
    return z;
}