int fib(int x) {
    if (x < 2)
        return 1;
    return fib(x - 1) + fib(x - 2);
}


int main() {
    int x = fib(5);
    return x;
}