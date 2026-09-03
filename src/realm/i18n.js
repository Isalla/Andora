// Server-seitiger i18n. Gleiche i18n/*.json wie der Godot-Client.
// const { t } = require("./i18n.js"); t("de", "boss_spawned");
const fs = require("fs");
const path = require("path");

const LANGS = ["en", "de"];
const tables = {};

function loadAll() {
  const dir = path.join(__dirname, "..", "..", "i18n");
  for (const lang of LANGS) {
    try {
      const txt = fs.readFileSync(path.join(dir, lang + ".json"), "utf8");
      tables[lang] = JSON.parse(txt);
    } catch (e) {
      // optional, skip
    }
  }
}
loadAll();

function t(lang, key, args = {}) {
  const table = (lang && tables[lang] && tables[lang][key] !== undefined)
    ? tables[lang] : tables["en"];
  if (!table) return key;
  let out = table[key] !== undefined ? table[key] : key;
  for (const k of Object.keys(args)) {
    out = out.split("{" + k + "}").join(String(args[k]));
  }
  return out;
}

// i18n-Key-Übersetzung für einen Nachrichtentyp (z.B. "player_damage")
// + Argumente vom Server (entity-Name, Zahlen)
function localizedMessage(lang, key, args) {
  return t(lang, key, args);
}

module.exports = { t, localizedMessage, LANGS, reload: loadAll };
