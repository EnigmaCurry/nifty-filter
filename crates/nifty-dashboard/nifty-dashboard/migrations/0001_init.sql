-- identity providers

CREATE TABLE identity_provider (
    id              INTEGER PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,  -- e.g. 'traefik-forwardauth', 'google', 'github'
    display_name    TEXT NOT NULL         -- human-readable
    -- you can add is_default INTEGER DEFAULT 0 CHECK (is_default IN (0,1)) if you like
);

-- match ids from enum models::identity_provider::IdentityProviders
INSERT INTO identity_provider (id, name, display_name)
VALUES
    (1, 'System','System User'),
    (2, 'ForwardAuth', 'Traefik ForwardAuth'),
    (3, 'Oidc', 'OpenID Connect');

-- signup methods

CREATE TABLE signup_method (
    id              INTEGER PRIMARY KEY,
    code            TEXT NOT NULL UNIQUE,  -- 'self', 'admin', 'import', 'api', ...
    description     TEXT NOT NULL
);

INSERT INTO signup_method (code, description) VALUES
    ('self',  'User registered via web UI'),
    ('admin', 'Admin created the account manually');


-- users

CREATE TABLE [user] (
    id                   INTEGER PRIMARY KEY,
    identity_provider_id INTEGER NOT NULL REFERENCES identity_provider(id),
    external_id          TEXT NOT NULL,   -- value from ForwardAuth (e.g. subject or email)

    username             TEXT,            -- user-chosen username; see CHECK below

    is_registered        INTEGER NOT NULL DEFAULT 0
                         CHECK (is_registered IN (0,1)),
    registered_at        DATETIME DEFAULT (CURRENT_TIMESTAMP), -- auto-populate when omitted
    signup_method_id     INTEGER REFERENCES signup_method(id),

    status               TEXT NOT NULL DEFAULT 'active'
                         CHECK (status IN ('active','disabled','banned')),

    created_at           DATETIME NOT NULL DEFAULT (CURRENT_TIMESTAMP),
    updated_at           DATETIME NOT NULL DEFAULT (CURRENT_TIMESTAMP),

    CHECK (
        (is_registered = 0 AND username IS NULL)
        OR
        (is_registered = 1 AND username IS NOT NULL)
    ),

    UNIQUE(external_id),
    UNIQUE(username)
);

-- passwords

CREATE TABLE user_password (
    user_id       INTEGER NOT NULL
                  PRIMARY KEY
                  REFERENCES [user](id)
                  ON DELETE CASCADE,

    -- Full bcrypt string, e.g. "$2b$12$..."
    password_hash TEXT NOT NULL,

    -- When the password was first set
    created_at    DATETIME NOT NULL DEFAULT (CURRENT_TIMESTAMP),

    -- When the password was last changed
    updated_at    DATETIME NOT NULL DEFAULT (CURRENT_TIMESTAMP),

    -- Flag to force a password change on next login
    must_reset    INTEGER NOT NULL DEFAULT 0
                  CHECK (must_reset IN (0,1))
);

-- roles

CREATE TABLE [role] (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,        -- 'unregistered', 'user', 'admin', ...
    description TEXT NOT NULL
);

INSERT INTO [role] (name, description) VALUES
    ('user',         'Default registered user'),
    ('admin',        'Administrator');


-- user_role
-- IMPORTANT: user_id and assigned_by must match the type of user.id

CREATE TABLE user_role (
    user_id         INTEGER NOT NULL REFERENCES [user](id) ON DELETE CASCADE,
    role_id         INTEGER NOT NULL REFERENCES [role](id),
    assigned_at     DATETIME NOT NULL DEFAULT (CURRENT_TIMESTAMP),
    assigned_by     TEXT REFERENCES [user](id), -- which admin gave them this role

    PRIMARY KEY (user_id, role_id)
);

