import sys
if False:
    print("a")
else:
    print("b")
if True:
    print("c")
elif sys.argv:
    print("d")
    print("e")
else:
    print("f")
    print("g")
if "":
    print("h")
