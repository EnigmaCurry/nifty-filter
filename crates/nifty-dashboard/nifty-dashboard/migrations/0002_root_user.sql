-- root user (system identity provider)
-- Note: we let SQLite default registered_at to CURRENT_TIMESTAMP.

INSERT INTO [user] (
    id,
    identity_provider_id,
    external_id,
    username,
    is_registered,
    signup_method_id,
    status
)
VALUES (
    1,
    (SELECT id FROM identity_provider WHERE name = 'System'),
    'root',                        -- external_id
    'root',                        -- username
    1,                             -- is_registered
    (SELECT id FROM signup_method WHERE code = 'admin'),
    'active'
);

-- root is an admin, with system/system root user
INSERT INTO user_role (user_id, role_id, assigned_by)
SELECT u.id, r.id, NULL
FROM [user] u
JOIN identity_provider idp ON idp.id = u.identity_provider_id
JOIN [role] r ON r.name = 'admin'
WHERE u.external_id = 'root'
  AND idp.name = 'System';
