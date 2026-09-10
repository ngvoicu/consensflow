# Review: skilluri pe rol și baza tehnică

2026-09-09. **Verdict: skillul actual trebuie înlocuit cu două texte pe rol;
nu este suficientă redenumirea. Recomand păstrarea Tauri și refacerea concentrată
a logicii defecte și a interfeței.** Este o recomandare de planificare, nu o
rescriere autorizată sau o declarație că aplicația instalată a fost reparată.

## Ce am verificat în skill

Citite: generatorul `src/skill.js`, copia generată `skill/SKILL.md`, copia
instalată `~/.codex/skills/consensflow/SKILL.md`, instalarea și sincronizarea.
Constatările de mai jos privesc aceste copii, nu presupun că toate skillurile
instalate în celelalte harness-uri sunt identice. P1 afectează rolul/execuția;
P2 este ambiguitate sau mentenanță care trebuie eliminată din noul contract.

| Prioritate | Dovadă | Problemă și corecție propusă |
|---|---|---|
| P1 | `src/skill.js:60`, `src/install.js:25` și `:195` | Numele generic, descrierea foarte largă și instalarea globală expun coordonarea inclusiv în afara aplicației și workerilor. Generăm `consensflow-lead` numai pentru lead și `consensflow-pm` numai pentru PM; copiile globale vechi le ștergem noi manual, fără funcție de curățare în aplicație, conform clarificării lui Gabriel. |
| P1 | Copia instalată `:149–168`; `src/skill.js:214–240`; `src/sync.js:138–150` | Copia instalată cere să revii la următorul mesaj al utilizatorului și spune că Claude nu are rută nativă; sursa curentă diferă. Auto-heal compară hashul rosterului, deci un roster neschimbat nu actualizează singur proza. `staleSkills` detectează diferența, fără s-o rezolve. Conținutul privat trebuie legat de buildul aplicației care lansează rolul. |
| P1 | `src/skill.js:142–145`, `:163–176`, `:212–218`, `:249–254` | Timeoutul de peste zece minute și „thinking streams” descriu vechiul flux sincron. „Never wait”, „continue” și „stop/report before anything else” coexistă. Noul text separă dispatchul asincron, munca independentă, așteptarea unei dependențe și folosirea rezultatului primit. |
| P1 | `src/skill.js:228–231` față de `:255–256` | Regula finală permite schimbarea agentului dacă a eșuat comanda, deși un timeout/read error nu dovedește că taskul n-a fost admis. Cerem dovada neadmiterii sau alegerea explicită a utilizatorului cunoscând incertitudinea; `0 runs` nu este probă. |
| P2 | `src/skill.js:69–70`; `src/panes.js:285`, `:1004`, `:1134` | „Current working directory” promite implicit că un `cd` al leadului schimbă proiectul workerului. Lansarea folosește folderul sesiunii ConsensFlow. Noul text numește acel folder explicit. |
| P2 | `src/skill.js:168–194`, `:235–245`, `:278–280` | Regula de citire și excepția rezultatului livrat în părți sunt dispersate. Detaliile despre rute native și comanda de reparare globală a skillului fac leadul administrator de integrare. Le înlocuim cu un tabel de patru situații; un roster lipsă poate fi consultat o dată, fără instalare. Recuperarea cerută explicit și citirea unui raport furnizat nu sunt polling. |
| P2 | `src/skill.js:46–47`, `:3–7`, `:126–139` | Generatorul refuză rosterul gol, comentariul susține că numai rosterul variază, iar opțiunile sunt prezentate prea larg ca fiind combinabile. Rolurile trebuie să existe și fără workeri; păstrăm alternativele CLI explicite și scoatem istoricul din instrucțiunile operaționale. |

Corecții deja prezente în sursa actuală, de păstrat: continuarea muncii
independente, folosirea rezultatului livrat fără cerere suplimentară,
`cf read` direct când conversația este cunoscută, citirea tuturor părților și
interdicția de a deduce eșecul din `0 runs`. Acestea nu dovedesc livrarea
funcțională în aplicația alpha.34 instalată.

## Texte propuse pentru review

- [consensflow-lead](draft-skills/consensflow-lead/SKILL.md): delegare,
  continuare în aceeași conversație sau conversație nouă, rezultate și stări factuale.
- [consensflow-pm](draft-skills/consensflow-pm/SKILL.md): research și specificații,
  fără implementare/delegare; trimitere și citire de la lead, separat, numai la cerere.

Aceste fișiere sunt **drafturi în `.specs/`**, nu skilluri instalate sau încărcate
în procese. Comenzile PM sunt propuse, încă inexistente. Descrierile rămân scurte;
rosterul dinamic poate fi adăugat numai corpului skillului de lead la generare.
Noi ștergem manual vechiul `consensflow`; aplicația nu îl modifică și nu îl
regenerează ca alias. Cele două skilluri private se actualizează cu aplicația.
Backend-ul verifică rolul și sesiunea; numele unui skill nu oferă autoritate.

Acceptare înainte de livrare: pe cele patru harness-uri, leadul folosește un
rezultat automat fără tool call și continuă lucrul autorizat; cu rezultat absent
așteaptă fără polling; la cerere citește direct; pentru un rezultat mare citește
toate părțile aceleiași livrări; la timeout nu dublează taskul. PM planifică fără
agenți, „aprob planul” nu trimite nimic, iar „trimite” nu activează o citire.
Testăm și roster gol/modificat, upgrade fără schimbare de roster, resume,
skilluri globale existente păstrate identic de produs și absența ambelor roluri
la worker/în afara aplicației după curățarea manuală. Testele de text singure
nu demonstrează aceste comportamente.

## Ce folosește aplicația acum

| Funcție | Implementare verificată |
|---|---|
| Fereastră desktop | Tauri 2, Rust; `app/src-tauri/Cargo.toml:27`. Pe macOS folosește WKWebView. |
| Pane terminal | xterm.js 6 și FitAddon; `app/package.json`, `app/ui/term.js:1–29`. Nu Ghostty; în acest adaptor nu este încărcat un addon WebGL. |
| Procese native și PTY | Rust `portable-pty` 0.9, `app/src-tauri/src/pty.rs:9`. |
| Coordonare și persistență | Codul Node din `src/`; buildul include Node, `app/scripts/prepare-sidecar.mjs:83–86`. |

Acestea sunt două alegeri distincte: Tauri/Electron pentru aplicație și
xterm.js/Ghostty pentru terminal. Schimbarea primei nu o impune pe a doua.

Clarificare ulterioară Gabriel: **nicio extensie ConsensFlow în Pi**, nici pe
proces. Extensia actuală din `hosts/pi-extension/` și dependențele ei de launch,
editor, mesaje și finalizare trebuie înlocuite conform secțiunii 8 din plan.
Mecanismul fără extensie rămâne de demonstrat; rezultatele vechilor teste cu
extensia nu certifică noul contract. Skillurile pe rol sunt documente Markdown.

## Tauri, Electron sau Ghostty

Tauri folosește WebKit/WKWebView pe macOS. Electron include Chromium și Node
în aplicație, controlând versiunea motorului web independent de OS. Asta
înseamnă un compromis între distribuție și controlul rendererului, nu o dovadă
că unul desenează pane-urile noastre mai repede. ConsensFlow include deja Node,
deci nu trebuie prezentat ca o aplicație exclusiv Rust. Surse:
[Tauri WebView](https://v2.tauri.app/reference/webview-versions/),
[Electron: rationale și compromisuri](https://www.electronjs.org/docs/latest/why-electron).

Ghostty are un motor nativ și un proiect de integrare reutilizabil.
Documentația curentă a repository-ului spune că `libghostty-vt` este utilizabil
din C/Zig, inclusiv pe macOS și WebAssembly, dar interfața încă se schimbă.
Acesta oferă parsarea și starea terminalului; un import nu înlocuiește singur
xterm.js cu întregul renderer Metal al aplicației Ghostty. Integrarea grafică,
inputul, selecția, clipboardul și layoutul trebuie proiectate și verificate.
Surse: [repository Ghostty](https://github.com/ghostty-org/ghostty#cross-platform-libghostty-for-embeddable-terminals),
[interfața VT și render state](https://github.com/ghostty-org/ghostty/blob/main/include/ghostty/vt.h).

**Recomandare:** rămânem pe Tauri și xterm.js pentru etapa propusă; refacem
interfața cerută și logica de coordonare care a produs defectele. Citirea unui
rezultat depinde acum de transcriptul leadului (`src/delivery-watch.js:430`),
iar exitul poate elimina paneul workerului (`src/panes.js:1397`). Aceste legături
nu sunt impuse de Tauri sau xterm.js. O rescriere în Electron le-ar putea copia.

Aspectul nou poate avea ierarhie clară, spațiere și controale coerente, cu
lead/PM/workeri distincte; terminalele păstrează stilul nativ al harness-urilor.
Nu promitem că un renderer diferit rezolvă singur textul alb. Verificăm întâi
secvențele de culoare, mediul PTY și răspunsurile terminalului; comparația vizuală
folosește aceeași configurație nativă. `term.js:36` folosește un semnal intern
xterm pentru separarea inputului uman de răspunsurile emulatorului: este o
dependență de mentenanță de testat, nu o dovadă de defect vizual.

Nu avem un benchmark comparativ pentru această aplicație. Dacă persistă un
blocaj demonstrat al rendererului, comparăm izolat 1, 5 și 20 pane-uri plus PM,
pe aceeași mașină, același flux de output și același scrollback: timp de pornire,
memorie totală a proceselor, CPU în repaus și sub output, latența tastării și
fluiditatea scrollului. Un rezultat clar poate justifica apoi Electron sau un
prototip Ghostty. Acea comparație nu inspectează versiuni de harness.

## Ce înseamnă curățare în acest plan

Skillul global vechi îl ștergem noi manual la trecerea controlată. Din produs
scoatem instalarea lui și orice ștergere/migrare automată; noile skilluri rămân
administrate privat. Eliminăm extensia Pi și dependențele ei, cu înlocuitor
verificat, precum și toate controalele de versiune nativă,
textele/rutele care nu mai fac parte din contract și codul
devenit inutil prin aceste schimbări. Un singur traseu de citire a rezultatului
trebuie servit atât manual, cât și automat, separat de confirmarea primirii;
istoricul conversației persistă separat de PTY. Migrarea păstrează răspunsurile
existente. Curățarea nu înseamnă ștergerea sesiunilor, datelor sau profilurilor.

Ordinea și probele sunt în [plan](plan-session-view-app-capabilities.md): rezultate
și restart, roluri și administrare privată, layout/PM, apoi candidat integrat. README, help și
metadatele de distribuție trebuie să descrie același contract; recordurile vechi
din TDD Log rămân istoric, etichetate unde sunt depășite. Modificarea stackului
rămâne o opțiune discutată, fără implementare autorizată în acest tur.
