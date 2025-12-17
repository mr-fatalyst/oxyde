# FastAPI Project

Complete example of using Oxyde with FastAPI.

## Setup

```bash
pip install oxyde fastapi uvicorn
```

## Project Structure

```
myapp/
├── main.py
├── models.py
└── routes.py
```

## Models

```python
# models.py
from oxyde import OxydeModel, Field

class User(OxydeModel):
    id: int | None = Field(default=None, db_pk=True)
    name: str
    email: str = Field(db_unique=True)
    is_active: bool = True

    class Meta:
        is_table = True
        table_name = "users"

class Post(OxydeModel):
    id: int | None = Field(default=None, db_pk=True)
    title: str
    content: str
    author: "User" | None = Field(default=None)

    class Meta:
        is_table = True
        table_name = "posts"
```

## Application

```python
# main.py
from fastapi import FastAPI
from oxyde import db

from routes import router

app = FastAPI(
    lifespan=db.lifespan(default="sqlite:///app.db")
)
app.include_router(router)
```

Before first run, create tables with migrations:

```bash
oxyde makemigrations
oxyde migrate
```

## Routes

```python
# routes.py
from fastapi import APIRouter, HTTPException
from pydantic import BaseModel

from models import User, Post

router = APIRouter()

# --- Schemas ---

class UserCreate(BaseModel):
    name: str
    email: str

class PostCreate(BaseModel):
    title: str
    content: str
    author_id: int

# --- Users ---

@router.get("/users")
async def list_users():
    return await User.objects.all()

@router.get("/users/{id}")
async def get_user(id: int):
    user = await User.objects.get_or_none(id=id)
    if not user:
        raise HTTPException(404, "User not found")
    return user

@router.post("/users", status_code=201)
async def create_user(data: UserCreate):
    return await User.objects.create(**data.model_dump())

@router.patch("/users/{id}")
async def update_user(id: int, data: UserCreate):
    user = await User.objects.get_or_none(id=id)
    if not user:
        raise HTTPException(404, "User not found")
    user.name = data.name
    user.email = data.email
    await user.save()
    return user

@router.delete("/users/{id}", status_code=204)
async def delete_user(id: int):
    count = await User.objects.filter(id=id).delete()
    if not count:
        raise HTTPException(404, "User not found")

# --- Posts ---

@router.get("/posts")
async def list_posts():
    return await Post.objects.all()

@router.get("/users/{user_id}/posts")
async def list_user_posts(user_id: int):
    return await Post.objects.filter(author_id=user_id).all()

@router.post("/posts", status_code=201)
async def create_post(data: PostCreate):
    # Verify author exists
    if not await User.objects.filter(id=data.author_id).exists():
        raise HTTPException(400, "Author not found")
    return await Post.objects.create(**data.model_dump())
```

## Run

```bash
uvicorn main:app --reload
```

API available at `http://localhost:8000/docs`

## With Transactions

```python
from oxyde.db import transaction

@router.post("/users/with-post", status_code=201)
async def create_user_with_post(data: UserCreate, post_title: str):
    async with transaction.atomic():
        user = await User.objects.create(**data.model_dump())
        post = await Post.objects.create(
            title=post_title,
            content="First post!",
            author_id=user.id
        )
    return {"user": user, "post": post}
```

## With Pagination

```python
@router.get("/posts/paginated")
async def list_posts_paginated(page: int = 1, per_page: int = 10):
    offset = (page - 1) * per_page
    posts = await Post.objects.order_by("-id").offset(offset).limit(per_page).all()
    total = await Post.objects.count()
    return {
        "items": posts,
        "total": total,
        "page": page,
        "pages": (total + per_page - 1) // per_page
    }
```

## Next Steps

- [Queries](../guide/queries.md) — Query API
- [Filtering](../guide/filtering.md) — Filter conditions
- [Transactions](../guide/transactions.md) — Atomic operations
