# Benchmarks

!!! info "Benchmark Date"
    These benchmarks were run on **January 23, 2026** with Oxyde 0.3.1.
    Results may vary depending on hardware and database configuration.

## Summary (average ops/sec)

### PostgreSQL

| Rank | ORM | Avg ops/sec |
|------|-----|-------------|
| 1 | Oxyde | 923.7 |
| 2 | Tortoise | 747.6 |
| 3 | Piccolo | 745.9 |
| 4 | SQLAlchemy | 335.6 |
| 5 | SQLModel | 324.0 |
| 6 | Peewee | 61.0 |
| 7 | Django | 58.5 |

### MySQL

| Rank | ORM | Avg ops/sec |
|------|-----|-------------|
| 1 | Oxyde | 1037.0 |
| 2 | Tortoise | 1019.2 |
| 3 | SQLAlchemy | 434.1 |
| 4 | SQLModel | 420.1 |
| 5 | Peewee | 370.5 |
| 6 | Django | 312.8 |

### SQLite

| Rank | ORM | Avg ops/sec |
|------|-----|-------------|
| 1 | Tortoise | 1476.6 |
| 2 | Oxyde | 1232.0 |
| 3 | Peewee | 449.4 |
| 4 | Django | 434.0 |
| 5 | SQLAlchemy | 341.5 |
| 6 | SQLModel | 336.3 |
| 7 | Piccolo | 295.1 |

---

## PostgreSQL Results

### CRUD Operations

![PostgreSQL CRUD](../img/benchmarks/postgresql_crud.png)

| Test | Oxyde | Django | SQLAlchemy | Tortoise | Piccolo | Peewee | SQLModel |
|------|-------|--------|------------|----------|---------|--------|----------|
| insert_single | 1685.7 | 121.8 | 747.7 | 1828.6 | 1748.3 | 111.0 | 684.3 |
| insert_bulk_100 | 290.0 | 84.8 | 189.0 | 325.9 | 241.0 | 90.7 | 133.1 |
| select_pk | 2979.7 | 121.0 | 921.7 | 2275.7 | 2462.1 | 135.7 | 897.0 |
| select_filter | 182.6 | 71.2 | 156.7 | 107.5 | 134.6 | 78.5 | 132.7 |
| update_single | 2310.8 | 120.5 | 707.5 | 1854.4 | 1968.0 | 116.5 | 681.7 |
| update_bulk | 1515.9 | 114.6 | 680.5 | 1506.3 | 1659.3 | 131.3 | 652.6 |
| delete_single | 354.5 | 69.5 | 290.8 | 367.3 | 365.0 | 96.4 | 281.7 |

### Query Operations

![PostgreSQL Queries](../img/benchmarks/postgresql_queries.png)

| Test | Oxyde | Django | SQLAlchemy | Tortoise | Piccolo | Peewee | SQLModel |
|------|-------|--------|------------|----------|---------|--------|----------|
| filter_simple | 2472.4 | 119.8 | 894.2 | 2230.7 | 2283.3 | 120.5 | 882.2 |
| filter_complex | 351.0 | 88.6 | 259.5 | 207.0 | 258.7 | 84.9 | 233.1 |
| filter_in | 340.5 | 113.0 | 491.8 | 572.4 | 651.8 | 83.5 | 450.4 |
| order_limit | 2417.8 | 120.2 | 843.7 | 1506.9 | 1900.3 | 113.2 | 849.0 |
| aggregate_count | 3516.2 | 119.6 | 964.3 | 2564.4 | 2541.0 | 130.9 | 954.1 |
| aggregate_mixed | 2587.6 | 110.4 | 857.6 | 1820.4 | 868.3 | 127.3 | 898.2 |

### Relations

![PostgreSQL Relations](../img/benchmarks/postgresql_relations.png)

| Test | Oxyde | Django | SQLAlchemy | Tortoise | Piccolo | Peewee | SQLModel |
|------|-------|--------|------------|----------|---------|--------|----------|
| join_simple | 5.4 | 3.0 | 4.3 | 3.2 | 2.0 | 3.6 | 3.7 |
| join_filter | 5.8 | 3.2 | 4.5 | 3.4 | 2.1 | 3.7 | 3.9 |
| prefetch_related | 5.4 | 4.0 | 3.7 | 4.6 | 4.9 | 5.2 | 2.9 |
| nested_prefetch | 2.0 | 1.1 | 1.8 | 2.9 | 3.1 | 4.2 | 1.6 |

### Concurrent Operations

![PostgreSQL Concurrent](../img/benchmarks/postgresql_concurrent.png)

| Concurrency | Oxyde | Django | SQLAlchemy | Tortoise | Piccolo | Peewee | SQLModel |
|-------------|-------|--------|------------|----------|---------|--------|----------|
| 10 | 548.6 | 8.7 | 10.0 | 388.2 | 396.2 | 13.8 | 10.0 |
| 25 | 247.8 | 3.8 | 5.0 | 153.9 | 168.3 | 5.1 | 4.9 |
| 50 | 126.8 | 1.9 | 4.6 | 78.8 | 88.8 | 2.6 | 4.5 |
| 100 | 64.2 | 0.9 | 4.0 | 40.3 | 44.5 | 1.3 | 3.9 |
| 200 | 31.3 | 0.5 | 3.1 | 19.7 | 21.6 | 0.6 | 3.0 |

### Scalability

![PostgreSQL Scalability](../img/benchmarks/postgresql_scalability.png)

---

## MySQL Results

### CRUD Operations

![MySQL CRUD](../img/benchmarks/mysql_crud.png)

| Test | Oxyde | Django | SQLAlchemy | Tortoise | Peewee | SQLModel |
|------|-------|--------|------------|----------|--------|----------|
| insert_single | 2118.9 | 863.3 | 956.6 | 2754.4 | 934.3 | 977.0 |
| insert_bulk_100 | 481.6 | 241.4 | 155.7 | 365.1 | 236.7 | 123.6 |
| select_pk | 3269.4 | 904.6 | 1155.6 | 3350.3 | 925.6 | 1195.4 |
| select_filter | 196.3 | 146.3 | 80.7 | 66.3 | 82.4 | 78.2 |
| update_single | 2460.6 | 686.5 | 979.3 | 2556.2 | 947.1 | 917.0 |
| update_bulk | 1705.7 | 653.5 | 880.0 | 2011.3 | 828.2 | 878.2 |
| delete_single | 2046.8 | 298.0 | 987.0 | 2335.6 | 867.2 | 951.0 |

### Query Operations

![MySQL Queries](../img/benchmarks/mysql_queries.png)

### Relations

![MySQL Relations](../img/benchmarks/mysql_relations.png)

### Concurrent Operations

![MySQL Concurrent](../img/benchmarks/mysql_concurrent.png)

### Scalability

![MySQL Scalability](../img/benchmarks/mysql_scalability.png)

---

## SQLite Results

### CRUD Operations

![SQLite CRUD](../img/benchmarks/sqlite_crud.png)

| Test | Oxyde | Django | SQLAlchemy | Tortoise | Piccolo | Peewee | SQLModel |
|------|-------|--------|------------|----------|---------|--------|----------|
| insert_single | 3046.0 | 589.3 | 670.2 | 1893.3 | 466.5 | 605.1 | 623.2 |
| insert_bulk_100 | 631.6 | 260.4 | 31.9 | 448.3 | 176.4 | 259.4 | 29.6 |
| select_pk | 3252.1 | 1386.6 | 960.2 | 4958.9 | 913.6 | 1548.8 | 941.9 |
| select_filter | 175.0 | 181.6 | 161.9 | 76.0 | 109.8 | 70.3 | 137.9 |
| update_single | 3720.8 | 625.8 | 639.8 | 1870.0 | 507.4 | 594.7 | 649.3 |
| update_bulk | 2485.0 | 1419.5 | 916.8 | 5477.1 | 978.7 | 1703.1 | 925.0 |
| delete_single | 222.6 | 88.7 | 155.6 | 183.5 | 120.4 | 134.7 | 156.0 |

### Query Operations

![SQLite Queries](../img/benchmarks/sqlite_queries.png)

### Relations

![SQLite Relations](../img/benchmarks/sqlite_relations.png)

### Concurrent Operations

![SQLite Concurrent](../img/benchmarks/sqlite_concurrent.png)

### Scalability

![SQLite Scalability](../img/benchmarks/sqlite_scalability.png)

---

## Latency (PostgreSQL)

### Mean Latency (ms)

| Test | Oxyde | Django | SQLAlchemy | Tortoise | Piccolo | Peewee | SQLModel |
|------|-------|--------|------------|----------|---------|--------|----------|
| insert_single | 0.59 | 8.21 | 1.34 | 0.55 | 0.57 | 9.01 | 1.46 |
| select_pk | 0.34 | 8.27 | 1.08 | 0.44 | 0.41 | 7.37 | 1.11 |
| update_single | 0.43 | 8.30 | 1.41 | 0.54 | 0.51 | 8.58 | 1.47 |

### P99 Latency (ms)

| Test | Oxyde | Django | SQLAlchemy | Tortoise | Piccolo | Peewee | SQLModel |
|------|-------|--------|------------|----------|---------|--------|----------|
| insert_single | 1.04 | 23.71 | 2.36 | 0.97 | 0.83 | 24.13 | 2.17 |
| select_pk | 0.55 | 24.95 | 2.16 | 0.74 | 0.84 | 9.18 | 1.91 |
| update_single | 0.59 | 20.27 | 2.51 | 0.69 | 0.69 | 24.41 | 1.76 |

---

## Memory Usage (PostgreSQL)

Peak memory (MB):

| Test | Oxyde | Django | SQLAlchemy | Tortoise | Piccolo | Peewee | SQLModel |
|------|-------|--------|------------|----------|---------|--------|----------|
| insert_single | 52.1 | 65.5 | 57.6 | 43.8 | 41.2 | 59.0 | 66.7 |
| select_pk | 58.7 | 71.0 | 63.1 | 48.1 | 46.0 | 64.1 | 69.8 |
| join_simple | 132.6 | 117.4 | 88.2 | 63.0 | 101.8 | 104.8 | 104.4 |
| nested_prefetch | 151.8 | 184.8 | 116.6 | 93.0 | 106.6 | 111.6 | 129.1 |

---

## Test Environment

| Parameter | Value |
|-----------|-------|
| CPU | Intel Core i7-11800H @ 2.30GHz |
| Cores | 2 |
| RAM | 4 GB |
| OS | Linux 6.8.0-90-generic |
| Python | 3.12.12 |
| Container | Docker |

### Package Versions

| Package | Version |
|---------|---------|
| oxyde | 0.3.1 |
| asyncpg | 0.31.0 |
| django | 6.0.1 |
| sqlalchemy | 2.0.46 |
| tortoise-orm | 0.25.3 |
| piccolo | 1.30.0 |
| peewee | 3.19.0 |
| sqlmodel | 0.0.31 |

### Test Data

- Users: 1000
- Posts per user: 20

---

## Methodology

Benchmarks are available in a separate repository: [oxyde-benchmarks](https://github.com/mr-fatalyst/oxyde-benchmarks)

**Configuration:**

- 100 iterations per test
- 10 warmup iterations
- Connection pool warmed up before measurements
- Each ORM tested with recommended async drivers

## Next Steps

- [Performance Tips](performance.md) — How to optimize your queries
- [Internals](internals.md) — Rust core architecture
