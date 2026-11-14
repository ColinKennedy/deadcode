class A:  # comment
    def unused(self):
        pass


class B:
    def unused(self):  # "#" inside a string: not a comment
        return "#:"


A(), B()


# def commented_out():
#     pass
