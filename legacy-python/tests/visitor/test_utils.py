import ast

from deadcode.visitor.utils import get_decorator_name

class TestUtils:
    def test_get_decorator_name_attribute(self):
        # Test with a decorator with an attribute
        decorator = ast.Attribute(value=ast.Name(id='module', ctx=ast.Load()), attr='my_decorator1')
        assert get_decorator_name(decorator) == '@module.my_decorator1'

        # Test with a complex decorator
        decorator = ast.Attribute(
            value=decorator,  # Create nested attributes
            attr='my_decorator2',
            ctx=ast.Load()
        )
        assert get_decorator_name(decorator) == '@module.my_decorator1.my_decorator2'


    def test_get_decorator_name_call(self):
        decorator = ast.Attribute(value=ast.Name(id='module', ctx=ast.Load()), attr='my_decorator')
        assert get_decorator_name(decorator) == '@module.my_decorator'

        # Test with a decorator that is a call
        decorator = ast.Call(
            func=decorator,
            args=[],
            keywords=[]
        )
        assert get_decorator_name(decorator) == '@module.my_decorator'

    def test_get_decorator_name_bare_name(self):
        # Test with a bare name decorator, e.g. @property, @staticmethod
        decorator = ast.Name(id='property', ctx=ast.Load())
        assert get_decorator_name(decorator) == '@property'
