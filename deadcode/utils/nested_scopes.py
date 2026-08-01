from typing import Any, Dict, List, Optional, Union

from deadcode.visitor.code_item import CodeItem

# Sentinel key used to store a scope's own CodeItem definitions in a dict
# separate from its nested child-scope dicts. A scope-part name (e.g. a class
# or function name) and a CodeItem's name can be the same string (a class
# named `Bar` nested under module `foo` is both a child scope `foo.Bar` and a
# CodeItem named `Bar` defined in scope `foo`), so they can't share one dict's
# key space without ambiguity. `object()` can't equal any string or CodeItem,
# so it can't collide with either.
_ITEMS = object()


class NestedScope:
    """This data structure is used to track what types are defined in each scope.

    It allows to correctly detect the type which is being used.

    TODO: This data structure could also be used for tracking the usages of types, but
    there is an issue: the usage could be registered before the definition.
    A mock structure which would hold the usage should used to store count of usages
    until the type is defined.
    """

    def __init__(self) -> None:
        self._scopes: Dict[Union[str, CodeItem, object], Any] = {}

    def add(self, code_item: CodeItem) -> None:
        """Adds code item to nested scope."""

        if code_item.scope is None:
            return None

        scope_parts = code_item.scope.split('.')

        current_scope = self._scopes
        for scope_part in scope_parts:
            if scope_part not in current_scope:
                current_scope[scope_part] = {}  # Could use None if type cannot have scope
            current_scope = current_scope[scope_part]

        # Store this scope's own definitions in their own dict (keyed by
        # _ITEMS) instead of mixing them into current_scope's child-scope
        # keys, so get() below can do a direct O(1) dict lookup by name
        # instead of scanning current_scope.keys() for a match.
        items: Dict[Union[str, CodeItem], CodeItem] = current_scope.setdefault(_ITEMS, {})
        items[code_item] = code_item

    def get(self, name: str, scope: str) -> Optional[Union[CodeItem, str]]:
        """Returns CodeItem which matches scoped_name (e.g. package.class.method.variable)
        from the given scope or None if its not found."""

        # TODO: investigate how modules in subdirectories are being collected into scopes.
        #   Is filename scope flat: meaning it forgets parent directories?
        #   File scope should be created using working path as base dir.
        #   We would get name collisions for this structure:
        #       projects.models, billing.models, auth.models: only one root scope called models would be registered.

        # Create a stack of scopes begining from nearest and following with parent one
        scopes: List[Dict[Union[str, CodeItem, object], Any]] = []
        next_scope = self._scopes
        for scope_part in scope.split('.'):
            if scope_part not in next_scope:
                return None
            next_scope = next_scope[scope_part]
            scopes.insert(0, next_scope)

        # Search for definition with provided name in scopes.
        # CodeItem.__hash__/__eq__ accept plain strings, so a dict.get() with
        # the string name directly finds the matching CodeItem key's value in
        # O(1) average time.
        for current_scope in scopes:
            found_items: Optional[Dict[Union[str, CodeItem], CodeItem]] = current_scope.get(_ITEMS)
            if found_items and (code_item := found_items.get(name)) is not None:
                return code_item

        return None

    def mark_as_used(self, name: str, scope: str) -> None:
        # >TODO: This method does not work, when methods are being invoked.
        # The scope is global, name is method name, but the instance and
        # class names are not being taken into account.

        # Solution: parsing of a method invocation should be handled differently.
        # More precicely. The parsing should be in the right order.

        # Next step to solve this issue:
        # > Investigate how usage statement is being handled and what can be done about it.
        # Write tests for Class and instance creations.

        # In this place I should get a list of names, which are being used in the invocation.

        # if name == "spam":
        #     breakpoint()

        code_item = self.get(name, scope)
        if isinstance(code_item, CodeItem):
            code_item.number_of_uses += 1
