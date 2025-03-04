int main() {
    int a = 1;
    {
        int b = a + 2;
        int c = b + 3;
    }
    int c = a * (a + a);
    return c;
}