"""Oxyde ORM Advanced Queries Example.

Demonstrates advanced query features:
- Q expressions for complex filters
- exclude() for negation
- Aggregates (Count, Sum, Avg, Max, Min)
- group_by() and having()
- annotate() for computed fields
- exists() for existence checks
- F expressions for field references
- order_by(), distinct(), limit(), offset()

Usage:
    export DATABASE_URL=sqlite://demo.db
    python examples/advanced_queries.py
"""

from __future__ import annotations

import asyncio
import os

from oxyde import (
    AsyncDatabase,
    Model,
    Field,
    Q,
    F,
    Count,
    Sum,
    Avg,
    Max,
    Min,
    disconnect_all,
    execute_raw,
)


# =============================================================================
# Model Definitions
# =============================================================================

class Product(Model):
    """Product model for demonstrating queries."""

    id: int | None = Field(default=None, db_pk=True)
    name: str
    category: str
    price: int  # in cents
    stock: int = 0
    is_active: bool = True

    class Meta:
        is_table = True
        table_name = "products"


class Order(Model):
    """Order model for demonstrating aggregates."""

    id: int | None = Field(default=None, db_pk=True)
    product_id: int
    quantity: int
    total: int  # in cents

    class Meta:
        is_table = True
        table_name = "orders"


# =============================================================================
# Database Setup
# =============================================================================

async def setup_tables(db: AsyncDatabase) -> None:
    """Create tables using raw SQL."""
    await execute_raw("""
        CREATE TABLE IF NOT EXISTS products (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            category TEXT NOT NULL,
            price INTEGER NOT NULL,
            stock INTEGER DEFAULT 0,
            is_active INTEGER DEFAULT 1
        )
    """, using=db.name)
    await execute_raw("""
        CREATE TABLE IF NOT EXISTS orders (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            product_id INTEGER NOT NULL REFERENCES products(id),
            quantity INTEGER NOT NULL,
            total INTEGER NOT NULL
        )
    """, using=db.name)


async def seed_data(db: AsyncDatabase) -> None:
    """Insert sample data."""
    # Clear existing data
    await Order.objects.filter().delete(using=db.name)
    await Product.objects.filter().delete(using=db.name)

    # Create products
    products = await Product.objects.bulk_create(
        [
            {"name": "Laptop", "category": "Electronics", "price": 99900, "stock": 10},
            {"name": "Mouse", "category": "Electronics", "price": 2500, "stock": 50},
            {"name": "Keyboard", "category": "Electronics", "price": 7500, "stock": 30},
            {"name": "Desk", "category": "Furniture", "price": 29900, "stock": 5},
            {"name": "Chair", "category": "Furniture", "price": 19900, "stock": 8},
            {"name": "Monitor", "category": "Electronics", "price": 34900, "stock": 0, "is_active": False},
        ],
        using=db.name,
    )

    # Create orders
    await Order.objects.bulk_create(
        [
            {"product_id": products[0].id, "quantity": 2, "total": 199800},
            {"product_id": products[1].id, "quantity": 5, "total": 12500},
            {"product_id": products[1].id, "quantity": 3, "total": 7500},
            {"product_id": products[2].id, "quantity": 1, "total": 7500},
            {"product_id": products[3].id, "quantity": 1, "total": 29900},
        ],
        using=db.name,
    )

    print(f"Seeded {len(products)} products and 5 orders")


# =============================================================================
# Main Demo
# =============================================================================

async def main() -> None:
    url = os.getenv("DATABASE_URL", "sqlite://demo.db")
    db = AsyncDatabase(url, name="default")
    await db.connect()

    try:
        await setup_tables(db)
        await seed_data(db)

        print("\n" + "=" * 60)
        print("Advanced Queries Demo")
        print("=" * 60)

        # ---------------------------------------------------------------------
        # Q EXPRESSIONS - Complex Filters
        # ---------------------------------------------------------------------
        print("\n1. Q Expressions (AND, OR, NOT)...")

        # OR condition: Electronics OR price < 200
        results = await Product.objects.filter(
            Q(category="Electronics") | Q(price__lt=20000)
        ).all(using=db.name)
        print(f"   Electronics OR cheap: {[p.name for p in results]}")

        # AND with OR: Active AND (Electronics OR Furniture with stock > 5)
        results = await Product.objects.filter(
            Q(is_active=True) & (Q(category="Electronics") | Q(stock__gt=5))
        ).all(using=db.name)
        print(f"   Complex filter: {[p.name for p in results]}")

        # NOT: Products not in Electronics
        results = await Product.objects.filter(
            ~Q(category="Electronics")
        ).all(using=db.name)
        print(f"   NOT Electronics: {[p.name for p in results]}")

        # ---------------------------------------------------------------------
        # EXCLUDE - Negation
        # ---------------------------------------------------------------------
        print("\n2. exclude() - Negation...")

        # All except inactive
        results = await Product.objects.exclude(is_active=False).all(using=db.name)
        print(f"   Exclude inactive: {[p.name for p in results]}")

        # Exclude multiple conditions
        results = await Product.objects.exclude(
            category="Electronics",
            stock__lt=10,
        ).all(using=db.name)
        print(f"   Exclude Electronics with low stock: {[p.name for p in results]}")

        # ---------------------------------------------------------------------
        # LOOKUPS - Field Operators
        # ---------------------------------------------------------------------
        print("\n3. Lookups (operators)...")

        # Range
        results = await Product.objects.filter(price__gte=5000, price__lte=30000).all(using=db.name)
        print(f"   Price $50-$300: {[p.name for p in results]}")

        # Contains (case-insensitive)
        results = await Product.objects.filter(name__icontains="o").all(using=db.name)
        print(f"   Name contains 'o': {[p.name for p in results]}")

        # In list
        results = await Product.objects.filter(category__in=["Electronics", "Furniture"]).all(using=db.name)
        print(f"   In categories: {[p.name for p in results]}")

        # ---------------------------------------------------------------------
        # AGGREGATES
        # ---------------------------------------------------------------------
        print("\n4. Aggregates...")

        # Count
        total = await Product.objects.count(using=db.name)
        print(f"   Total products: {total}")

        # Count with filter
        active_count = await Product.objects.filter(is_active=True).count(using=db.name)
        print(f"   Active products: {active_count}")

        # Sum
        total_stock = await Product.objects.filter(is_active=True).sum("stock", using=db.name)
        print(f"   Total stock (active): {total_stock}")

        # Avg
        avg_price = await Product.objects.filter(is_active=True).avg("price", using=db.name)
        print(f"   Average price (active): ${avg_price / 100:.2f}" if avg_price else "   No data")

        # Max/Min
        max_price = await Product.objects.max("price", using=db.name)
        min_price = await Product.objects.min("price", using=db.name)
        print(f"   Price range: ${min_price / 100:.2f} - ${max_price / 100:.2f}")

        # ---------------------------------------------------------------------
        # EXISTS
        # ---------------------------------------------------------------------
        print("\n5. exists() - Existence Check...")

        has_expensive = await Product.objects.filter(price__gt=50000).exists(using=db.name)
        print(f"   Has expensive products (>$500): {has_expensive}")

        has_cheap = await Product.objects.filter(price__lt=1000).exists(using=db.name)
        print(f"   Has cheap products (<$10): {has_cheap}")

        # ---------------------------------------------------------------------
        # F EXPRESSIONS - Field References
        # ---------------------------------------------------------------------
        print("\n6. F Expressions (field references)...")

        # Increment stock by 10
        updated = await Product.objects.filter(category="Electronics").update(
            stock=F("stock") + 10,
            using=db.name,
        )
        print(f"   Incremented stock for {updated} Electronics products")

        # Check updated values
        electronics = await Product.objects.filter(category="Electronics").all(using=db.name)
        print(f"   New stock levels: {[(p.name, p.stock) for p in electronics]}")

        # ---------------------------------------------------------------------
        # ORDER BY, DISTINCT, LIMIT, OFFSET
        # ---------------------------------------------------------------------
        print("\n7. Ordering and Pagination...")

        # Order by price descending
        expensive_first = await Product.objects.order_by("-price").limit(3).all(using=db.name)
        print(f"   Top 3 expensive: {[p.name for p in expensive_first]}")

        # Order by multiple fields
        sorted_products = await Product.objects.order_by("category", "-price").all(using=db.name)
        print(f"   By category, then price: {[(p.category, p.name) for p in sorted_products]}")

        # Pagination with offset
        page2 = await Product.objects.order_by("id").offset(2).limit(2).all(using=db.name)
        print(f"   Page 2 (2 items): {[p.name for p in page2]}")

        # Distinct categories
        categories = await Product.objects.values_list("category", flat=True).distinct().all(using=db.name)
        print(f"   Distinct categories: {categories}")

        # ---------------------------------------------------------------------
        # GROUP BY & HAVING (via annotate)
        # ---------------------------------------------------------------------
        print("\n8. Group By with Aggregates...")

        # Count products per category
        by_category = await Product.objects.annotate(
            product_count=Count("id")
        ).group_by("category").all(using=db.name)
        for row in by_category:
            print(f"   {row['category']}: {row['product_count']} products")

        print("\n" + "=" * 60)
        print("Done!")
        print("=" * 60)

    finally:
        await disconnect_all()


if __name__ == "__main__":
    asyncio.run(main())
