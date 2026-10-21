// Verifikasi provider Pact untuk lumbung-backend-service (LMB-BE-TEST-PACT).
// Memverifikasi berkas pact di ./pacts terhadap server yang sudah berjalan, dengan state handler yang
// menyiapkan data di DB uji lewat psql dan API itu sendiri. Hanya untuk DB UJI (state handler menghapus data).
//
// Variabel: PROVIDER_URL (default http://127.0.0.1:8080), DATABASE_URL (wajib, DB uji),
// SERVICE_KEY (X-Service-Key server, wajib), PACT_FILES (opsional, daftar berkas dipisah koma).
import { Verifier } from "@pact-foundation/pact";
import { execFileSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const akar = dirname(fileURLToPath(import.meta.url));
const providerUrl = process.env.PROVIDER_URL ?? "http://127.0.0.1:8080";
const databaseUrl = process.env.DATABASE_URL;
const kunciLayanan = process.env.SERVICE_KEY;
if (!databaseUrl || !kunciLayanan) {
  console.error("DATABASE_URL (DB uji) dan SERVICE_KEY wajib diisi");
  process.exit(2);
}

const psql = (sql) =>
  execFileSync("psql", [databaseUrl, "-v", "ON_ERROR_STOP=1", "-qc", sql], { stdio: ["ignore", "inherit", "inherit"] });

/** Senin minggu berjalan menurut WIB (YYYY-MM-DD). */
function seninWib() {
  const wib = new Date(new Date().toLocaleString("en-US", { timeZone: "Asia/Jakarta" }));
  const hari = (wib.getDay() + 6) % 7; // 0 = Senin
  wib.setDate(wib.getDate() - hari);
  const p = (n) => String(n).padStart(2, "0");
  return `${wib.getFullYear()}-${p(wib.getMonth() + 1)}-${p(wib.getDate())}`;
}

async function terbitkanPapan() {
  const res = await fetch(`${providerUrl}/v1/admin/papan-harga`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-service-key": kunciLayanan },
    body: JSON.stringify({
      uuid: "11111111-2222-4333-8444-555555555555",
      berlaku_mulai: seninWib(),
      referensi: [
        { komoditas: "cabai", harga: [35000, 36000, 35000] },
        { komoditas: "bawang", harga: [31200, 30000, 33000] },
        { komoditas: "kangkung", harga: [7000, 7500, 6800] },
      ],
    }),
  });
  // 201 baru, 200 duplikat (uuid sama), 409 minggu itu sudah terbit dengan uuid lain: semuanya berarti "ada".
  if (![200, 201, 409].includes(res.status)) {
    throw new Error(`state papan terbit gagal: ${res.status} ${await res.text()}`);
  }
}

const stateHandlers = {
  "papan harga minggu berjalan sudah terbit": async () => {
    await terbitkanPapan();
  },
  "papan harga minggu berjalan belum terbit": async () => {
    psql("DELETE FROM papan_harga");
  },
  "profil Lumbung tersedia": async () => {
    psql("UPDATE profil_publik SET deskripsi = 'Agregasi panen', kontak_kemitraan = 'mitra@lumbung.id' WHERE id = 1");
  },
  // Simulasi DB tidak tersedia: tabel yang dibaca endpoint diganti nama sementara (kueri gagal -> 500).
  "basis data tidak tersedia": {
    setup: async () => psql("ALTER TABLE profil_publik RENAME TO profil_publik_off"),
    teardown: async () => psql("ALTER TABLE profil_publik_off RENAME TO profil_publik"),
  },
};

const berkas = (process.env.PACT_FILES
  ? process.env.PACT_FILES.split(",")
  : readdirSync(join(akar, "pacts"))
      .filter((f) => f.endsWith(".json"))
      .map((f) => join(akar, "pacts", f))
).map((f) => resolve(f));

try {
  const keluaran = await new Verifier({
    provider: "lumbung-backend-service",
    providerBaseUrl: providerUrl,
    pactUrls: berkas,
    stateHandlers,
    logLevel: "warn",
  }).verifyProvider();
  console.log(keluaran);
  console.log("Verifikasi provider Pact: LULUS untuk", berkas.length, "berkas");
} catch (e) {
  console.error("Verifikasi provider Pact GAGAL:", e?.message ?? e);
  process.exit(1);
}
