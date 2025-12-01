-- Example schema for Oxyde ORM demo
-- Create a simple users table with basic fields and defaults.

CREATE TABLE IF NOT EXISTS public.users (
    id SERIAL PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    full_name TEXT NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    signup_ts TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE public.users IS 'Demo table used by the Oxyde ORM example.';
COMMENT ON COLUMN public.users.email IS 'Unique login / contact email.';
COMMENT ON COLUMN public.users.full_name IS 'Display name shown in the product.';

-- Widgets table for parameterized query example.
CREATE TABLE IF NOT EXISTS public.widgets (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE
);

COMMENT ON TABLE public.widgets IS 'Demo table for parameterized query showcase.';
COMMENT ON COLUMN public.widgets.slug IS 'Human readable handle used in URLs.';

-- Accounts table for transaction example.
CREATE TABLE IF NOT EXISTS public.accounts (
    id SERIAL PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    balance INTEGER NOT NULL DEFAULT 0
);

COMMENT ON TABLE public.accounts IS 'Demo table for transaction example.';
