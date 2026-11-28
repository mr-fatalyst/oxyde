"""Relation fields on model instances: reading, assigning, writing.

A relation field (``author: Author | None``, ``posts: list[Post]``) is a
regular Pydantic field, so its value lives in the instance ``__dict__`` and
Pydantic reads and writes it there directly (validation, ``model_dump``,
``repr``, equality).

Reading
    The accessors below sit on the model class as data descriptors and
    intercept only attribute *reads*. A relation that was not loaded raises
    ``RelationNotLoadedError`` instead of returning ``None`` / ``[]``, which
    would be indistinguishable from "no related row".

Assigning
    A foreign key has two faces: the related object (``author``) and its
    column (``author_id``). They are kept in step in both directions:
    the column follows the object when it is passed to the constructor
    (``fk_column_default``) or assigned (``sync_assignment``), and a loaded
    object is dropped when the column is pointed elsewhere.

Writing
    ``prepare_fk_columns`` runs before an INSERT/UPDATE: an object assigned
    before it was saved has its key by now, and one that is still unsaved is
    an error rather than a silent NULL.
"""

from __future__ import annotations

from collections.abc import Callable
from typing import TYPE_CHECKING, Any

from pydantic import BaseModel

from oxyde.exceptions import ManagerError, RelationNotLoadedError

if TYPE_CHECKING:
    from oxyde.models.base import Model
    from oxyde.models.metadata import ForeignKeyInfo


class ForeignKeyAccessor:
    """Read access to a foreign key relation (``post.author``).

    Without a key there is nothing to load, so the relation is ``None``.
    With a key, it is the loaded object or an error: never ``None``.
    """

    __slots__ = ("name", "column")

    def __init__(self, name: str, column: str) -> None:
        self.name = name
        self.column = column

    def __get__(self, instance: Model | None, owner: type | None = None) -> Any:
        if instance is None:
            # Not a class attribute: Pydantic probes the class for field
            # defaults and must keep finding none.
            raise AttributeError(self.name)
        values = instance.__dict__
        related = values.get(self.name)
        if related is None and values.get(self.column) is not None:
            raise RelationNotLoadedError(
                f"{type(instance).__name__}.{self.name} is not loaded. "
                f"Load it with .join('{self.name}'), or read "
                f"{self.column} for the key alone."
            )
        return related

    def __set__(self, instance: Model, value: Any) -> None:
        instance.__dict__[self.name] = value


class PrefetchAccessor:
    """Read access to a reverse or many-to-many relation (``author.posts``).

    The field defaults to an empty list, so "loaded" is tracked by the field
    being set: by ``prefetch()`` or by the caller.
    """

    __slots__ = ("name",)

    def __init__(self, name: str) -> None:
        self.name = name

    def __get__(self, instance: Model | None, owner: type | None = None) -> Any:
        if instance is None:
            raise AttributeError(self.name)
        if self.name not in instance.__pydantic_fields_set__:
            raise RelationNotLoadedError(
                f"{type(instance).__name__}.{self.name} is not loaded. "
                f"Load it with .prefetch('{self.name}')."
            )
        return instance.__dict__[self.name]

    def __set__(self, instance: Model, value: Any) -> None:
        set_loaded(instance, self.name, value)


def install_accessors(model: type[Model]) -> None:
    """Index the model's foreign keys and put the accessors on the class."""
    meta = model._db_meta
    meta.fk_fields = {}
    meta.fk_column_fields = {}
    for name, column in meta.field_metadata.items():
        if column.foreign_key is not None:
            fk = column.foreign_key
            meta.fk_fields[name] = fk
            meta.fk_column_fields[fk.column_name] = name
            setattr(model, name, ForeignKeyAccessor(name, fk.column_name))
        elif column.extra.get("reverse_fk") or column.extra.get("m2m"):
            setattr(model, name, PrefetchAccessor(name))


def set_loaded(instance: Model, name: str, value: Any) -> None:
    """Store a relation loaded by a query, bypassing assignment semantics."""
    instance.__dict__[name] = value
    instance.__pydantic_fields_set__.add(name)


def loaded(instance: Model, name: str) -> Any:
    """The stored value of a relation, without the not-loaded check."""
    return instance.__dict__.get(name)


def fk_column_default(relation: str, target_field: str) -> Callable[[dict], Any]:
    """Default of a nullable foreign key column: the key of the related
    object passed alongside it (``Post(author=author)``), else ``None``.

    Pydantic calls it with the fields validated so far, and only when the
    column itself was not given, so rows loaded from the database never
    reach it.
    """

    def default(data: dict[str, Any]) -> Any:
        related = data.get(relation)
        if isinstance(related, BaseModel):
            return getattr(related, target_field)
        return None

    return default


def sync_assignment(instance: Model, name: str, value: Any) -> None:
    """Keep a foreign key's object and column in step after an assignment.

    Assigning the object (or ``None``) sets the column. Assigning the column
    drops a loaded object that no longer matches it.
    """
    meta = type(instance)._db_meta
    fk = meta.fk_fields.get(name)
    if fk is not None:
        if value is None or isinstance(value, BaseModel):
            key = None if value is None else getattr(value, fk.target_field)
            instance.__dict__[fk.column_name] = key
            instance.__pydantic_fields_set__.add(fk.column_name)
        return

    relation = meta.fk_column_fields.get(name)
    if relation is not None:
        _drop_if_stale(instance, relation, meta.fk_fields[relation], value)


def _drop_if_stale(
    instance: Model, relation: str, fk: ForeignKeyInfo, key: Any
) -> None:
    related = instance.__dict__.get(relation)
    if related is not None and getattr(related, fk.target_field) != key:
        instance.__dict__[relation] = None


def prepare_fk_columns(instance: Model) -> None:
    """Settle foreign key columns before the instance is written.

    For each loaded related object:

    - it has no key: it was never saved, and writing would store NULL;
    - the column is empty: the object was assigned before it was saved, so
      the column takes its key now;
    - the column holds another key: the column wins, the object is dropped.
    """
    meta = type(instance)._db_meta
    for name, fk in meta.fk_fields.items():
        related = instance.__dict__.get(name)
        if related is None:
            continue
        key = getattr(related, fk.target_field)
        if key is None:
            raise ManagerError(
                f"Cannot save {type(instance).__name__}: its related "
                f"{type(related).__name__} in '{name}' is not saved yet "
                f"(no {fk.target_field}). Save it first."
            )
        current = instance.__dict__.get(fk.column_name)
        if current is None:
            instance.__dict__[fk.column_name] = key
            instance.__pydantic_fields_set__.add(fk.column_name)
        elif current != key:
            instance.__dict__[name] = None


__all__ = [
    "ForeignKeyAccessor",
    "PrefetchAccessor",
    "fk_column_default",
    "install_accessors",
    "loaded",
    "prepare_fk_columns",
    "set_loaded",
    "sync_assignment",
]
