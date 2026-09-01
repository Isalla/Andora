-- auth — Identitaet, Authentifizierung, Realm-/Server-Registrierung
-- Grundschema der auth-DB. Definiert hier nur die grundlegende Account-Tabelle;
-- das Anlegen der Datenbank liegt beim Betreiber (kein CREATE DATABASE).
-- Passwort-Hashing ist fuer Argon2id vorgesehen (password_hash VARCHAR(255)).
-- E-Mail-Adressen werden NICHT im Klartext gespeichert:
--   email_encrypted   = verschluesselte E-Mail (Schluessel getrennt von der DB)
--   email_lookup_hash = nicht reversibler Lookup-Hash (SHA-256, BINARY(32))
-- Sessions, Login-Tokens, Recovery-Tokens, Realm- und World-Server-Daten
-- werden in eigenen späteren Migrationen angelegt.
-- Diese Datei ist identisch zu migrations/001_accounts.sql gehalten.
USE auth;

CREATE TABLE IF NOT EXISTS accounts (
  id INT AUTO_INCREMENT PRIMARY KEY,
  username VARCHAR(32) NOT NULL UNIQUE,
  password_hash VARCHAR(255) NOT NULL,
  email_encrypted VARBINARY(512) NOT NULL,
  email_lookup_hash BINARY(32) NOT NULL UNIQUE,
  ban_until TIMESTAMP NULL,
  last_login_at TIMESTAMP NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
);
