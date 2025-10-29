// EXPECT: no errors
class MyClass {
    myProperty = 10

    function myMethod() {
        this.myProperty = 20;
        return this.myProperty;
    }

    function anotherMethod() {
        local result = this.myMethod();
        return result + this.myProperty;
    }
}

local obj = {
    x = 1
    y = 2

    sum = function() {
        return this.x + this.y;
    }
}
