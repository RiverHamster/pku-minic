int global = 2;
const int global_const = 1;

void f() {
    global = global + global_const;
}

int main() {
    f();
}