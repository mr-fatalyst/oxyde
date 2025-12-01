"""Basic tests for new API features."""

from __future__ import annotations

import pytest

from oxyde import Avg, Coalesce, Concat, Count, Max, Min, OxydeModel, Q, RawSQL, Sum


class SampleModel(OxydeModel):
    """Sample model for new features."""

    id: int
    name: str
    age: int | None = None
    status: str = "active"
    views: int = 0

    class Meta:
        is_table = True
        table_name = "sample_model"


def test_q_expressions():
    """Test Q expression creation."""
    q1 = Q(age__gte=18)
    q2 = Q(status="active")

    # AND
    combined_and = q1 & q2
    assert combined_and is not None

    # OR
    combined_or = q1 | q2
    assert combined_or is not None

    # NOT
    negated = ~q1
    assert negated is not None


def test_aggregates():
    """Test aggregate function creation."""
    count_agg = Count("id")
    assert count_agg.func_name == "count"

    sum_agg = Sum("views")
    assert sum_agg.func_name == "sum"

    avg_agg = Avg("age")
    assert avg_agg.func_name == "avg"

    max_agg = Max("age")
    assert max_agg.func_name == "max"

    min_agg = Min("age")
    assert min_agg.func_name == "min"


def test_concat():
    """Test Concat expression."""
    concat = Concat("first_name", "last_name", separator=" ")
    assert concat.func_name == "concat"
    ir = concat.to_ir()
    assert ir["func"] == "concat"
    assert ir["separator"] == " "


def test_coalesce():
    """Test Coalesce expression."""
    coalesce = Coalesce("nickname", "username", "email")
    assert coalesce.func_name == "coalesce"
    ir = coalesce.to_ir()
    assert ir["func"] == "coalesce"


def test_raw_sql():
    """Test RawSQL expression."""
    raw = RawSQL("COUNT(*) > 10")
    ir = raw.to_ir()
    assert ir["type"] == "raw"
    assert ir["sql"] == "COUNT(*) > 10"


def test_query_introspection():
    """Test sql() and query() methods."""
    query = SampleModel.objects.filter(age__gte=18)
    sql, params = query.sql()
    assert isinstance(sql, str)
    assert isinstance(params, list)

    query_ir = query.query()
    assert isinstance(query_ir, dict)
    assert query_ir["op"] == "select"


def test_query_methods():
    """Test that new query methods exist."""
    # Get the query builder via Manager.query()
    query = SampleModel.objects.filter()

    # Check methods exist
    assert hasattr(query, "exclude")
    assert hasattr(query, "exists")
    assert hasattr(query, "increment")
    assert hasattr(query, "annotate")
    assert hasattr(query, "group_by")
    assert hasattr(query, "having")
    assert hasattr(query, "sum")
    assert hasattr(query, "avg")
    assert hasattr(query, "max")
    assert hasattr(query, "min")
    assert hasattr(query, "union")
    assert hasattr(query, "union_all")
    assert hasattr(query, "explain")


def test_manager_methods():
    """Test that new manager methods exist."""
    manager = SampleModel.objects

    # Check manager methods
    assert hasattr(manager, "bulk_create")
    assert hasattr(manager, "filter")
    assert hasattr(manager, "all")
    assert hasattr(manager, "get")
    assert hasattr(manager, "create")

    # Note: update/delete/increment are on Query (via MutationMixin), accessed through filter()
    query = manager.filter(id=1)
    assert hasattr(query, "update")
    assert hasattr(query, "delete")
    assert hasattr(query, "increment")


def test_model_methods():
    """Test that new model methods exist."""
    # Check methods exist on class
    assert hasattr(SampleModel, "refresh")


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
