-- root user's local password: "password"
-- TODO: change this hardcoded dev password
-- bcrypt hash: $2a$12$CI.hSvuZLhl8I7et0Nkew.6UevgTB/cmaU./087Q1Svk2A/fqE.3C


INSERT INTO user_password (user_id, password_hash)
SELECT u.id,
       '$2a$12$CI.hSvuZLhl8I7et0Nkew.6UevgTB/cmaU./087Q1Svk2A/fqE.3C'
FROM [user] AS u
JOIN identity_provider AS p
  ON p.id = u.identity_provider_id
WHERE p.name = 'system'
  AND u.external_id = 'root'
ON CONFLICT(user_id) DO NOTHING;

INSERT INTO user_role (user_id, role_id, assigned_by)
SELECT u.id, r.id, NULL
FROM [user] u
JOIN identity_provider idp ON idp.id = u.identity_provider_id
JOIN [role] r ON r.name = 'admin'
WHERE u.external_id = 'ryan@rymcg.tech'
  AND idp.name = 'Oidc';
