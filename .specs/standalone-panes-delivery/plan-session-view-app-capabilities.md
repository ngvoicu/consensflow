> Updated decision, 2026-09-10: PM is above the lead in the sidebar and uses a
> full-size pane in the main ConsensFlow window. It is excluded from the session
> grid. This supersedes the separate-window proposal below; native identity and
> role permissions remain unchanged. Implemented in the alpha.39 candidate.

---
title: Lead fix, PM separat, capabilități pe rol și recuperarea rezultatelor
status: completed
created: 2026-09-09
updated: 2026-09-09
implementation_authorized: true
parent_spec: standalone-panes-delivery
---

# Plan implementat în alpha.37

Implementarea a fost făcută separat în `consensflow-next-ib1cg0zs`, fără agenți
ConsensFlow sau alți subagenți. După ce Gabriel a închis aplicația, a autorizat
reinstalarea și verificările native. Alpha.37 este instalată; sursa este integrată
în checkout-ul original, fără commit. Istoricul și profilurile native sunt păstrate.

Ultimele decizii prevalează asupra variantelor istorice: extensia Pi este aprobată,
privată procesului și pregătită numai dacă Pi este instalat; versiunile harness-urilor
sunt afișate informativ, fără să blocheze operațiile. Skillurile globale vechi au
fost șterse manual, după verificarea proprietății. Aplicația gestionează doar
skillurile private pentru lead/PM. Păstrăm Tauri/xterm.js.

Vezi [acceptarea alpha.37](acceptance-alpha37.md) pentru rezultate și limitele
externe de cotă/autentificare ale providerilor. Publicarea pe GitHub nu face parte
din instalarea locală autorizată.

## 1. Session view: lead fix și derulare orizontală a workerilor

Decizii confirmate de Gabriel:

- Pane-ul leadului rămâne fix în stânga, vizibil pe toată durata derulării.
- Are lățimea de două ori mai mare decât un pane de worker și ocupă întreaga
  înălțime disponibilă în session view.
- Numărăm leadul ca pane 1. Zona inițială cuprinde cel mult cinci pane-uri:
  leadul și patru workeri, pe două rânduri.
- PM-ul este separat de această vedere și nu intră în numerotarea grilei.
- Pane-ul 6 și următoarele sunt în coloane suplimentare spre dreapta.
  Numai zona workerilor are scroll orizontal. Numărul workerilor nu produce
  rânduri suplimentare sau scroll vertical al grilei.
- Scrollul din interiorul fiecărui terminal rămâne cel nativ, independent.

Propunere de ordonare: umplem fiecare coloană de sus în jos, apoi trecem la
următoarea. Numerele reprezintă ordinea vizuală, nu identificatorii globali p-N.

```text
          FIX                    ZONA WORKERILOR
┌──────────────────┐   ┌─────────┬─────────┐    ┌─────────┬─────────┐
│                  │   │ pane 2  │ pane 4  │ →  │ pane 6  │ pane 8  │
│   lead, pane 1   │   ├─────────┼─────────┤    ├─────────┼─────────┤
│    lățime 2×     │   │ pane 3  │ pane 5  │    │ pane 7  │ pane 9  │
└──────────────────┘   └─────────┴─────────┘    └─────────┴─────────┘
```

Reguli pentru cazurile mici și navigare:

- Fără workeri, leadul folosește tot spațiul. Cu unul sau doi workeri avem
  o coloană de workeri și raportul 2:1; un singur worker folosește înălțimea
  disponibilă, doi împart coloana. De la trei workeri avem două coloane
  vizibile și raportul de lățime 2:1:1. Celulele neocupate rămân libere.
- Adăugarea unui al cincilea worker nu micșorează pane-urile existente.
- Nu sărim automat la workerul nou dacă utilizatorul citește altceva.
  Alegerea explicită a unui worker din sidebar îl aduce în zona vizibilă;
  leadul rămâne pe loc.
- Păstrăm selecția și poziția de scroll per sesiune. Închiderea unui worker
  compactează ordinea și limitează scrollul la noua lungime.
- Focusarea explicită a unui singur pane rămâne o vedere separată, cu tot
  spațiul disponibil. Regula leadului fix se aplică numai session view.
- Lățimile se calculează din spațiul disponibil, inclusiv când sidebarul se
  restrânge. Înlocuim vechea regulă care crea rânduri pentru limita 260×180;
  nu schimbăm automat session view în focus view pe ferestre înguste.

Implementare vizată: `src/layout.js`, `app/ui/panes.js`,
`app/ui/index.html`. Refolosim instanțele Xterm și registry-ul existent;
derularea sau mutarea unui card nu recreează PTY-ul. Workerii din afara
ecranului continuă să consume/confirme outputul, să lucreze și să livreze.

Verificare: geometrie reală Playwright/Xterm pentru 1, 2, 3, 5, 6, 9 și 20
pane-uri; raport 2:1 cu toleranță de un pixel; poziția leadului neschimbată
după scroll; ultimul worker accesibil; fără overflow vertical al grilei;
redimensionare, sidebar, close, focus și două sesiuni simultane.

## 2. `consensflow-lead` este numai pentru leadurile din aplicație

Clarificare confirmată de Gabriel: skillul de coordonare se oferă numai
leadului, în cele patru harness-uri acceptate acum: Claude Code, OpenCode,
Pi și Codex. Kimi este exclus explicit din `LEAD_HARNESSES` în `src/tabs.js`
și nu apare în selectorul de lead din aplicație. Adăugarea Kimi ca lead
nu face parte din acest plan. PM-ul primește un skill diferit, descris în
secțiunea 4, prin aceleași mecanisme de încărcare limitate la procesul său.

Nume stabilite cu Gabriel: **`consensflow-lead`** pentru coordonare și
**`consensflow-pm`** pentru research/planificare. Numele generic `consensflow`
devine o instalare veche pe care o ștergem noi manual la trecerea la noua
aplicație, nu un al treilea skill sau alias activ gestionat de produs.
Numele identifică rolul; aplicația verifică separat drepturile procesului.

### Ce se întâmplă acum

`src/install.js` scrie `consensflow/SKILL.md` în directoarele globale definite
în `src/harnesses.js`. `src/sync.js` le actualizează la schimbarea rosterului.
De aceea și un agent pornit în cmux sau Terminal află numele agenților și
primește instrucțiuni despre `cf`, deși nu are contextul unui pane al aplicației.

Copia globală citită acum conține încă vechile reguli „Send and return” și
„Claude has no native route”. Generatorul din sursa alpha.36 conține deja
corecții. Simplul fapt că un fișier este înregistrat ca al nostru nu dovedește
că aparține versiunii aplicației folosite de sesiune.

### Direcția propusă

1. Generăm instrucțiunile și rosterul într-un director privat al ConsensFlow,
   în afara tuturor directoarelor descoperite automat de harness-uri și în
   afara proiectului utilizatorului. Conținutul aparține versiunii curente
   a aplicației; nu include chei sau tokenuri.
2. La fiecare pornire sau reluare a unui pane de lead, aplicația oferă acel
   conținut numai procesului respectiv, folosind opțiunile native ale harness-ului.
   Nu trimitem un mesaj uman artificial și nu pornim un tur al modelului
   doar pentru a anunța capabilitățile.
3. Unde există încărcare nativă de skill pe proces, o folosim. În celelalte
   cazuri folosim instrucțiuni suplimentare ale sesiunii cu o trimitere clară
   la documentul aplicației. Nu pretindem că acel caz este un skill în meniul
   nativ dacă harness-ul nu îl tratează astfel.
4. Leadul primește `consensflow-lead`, cu delegare/continuare/rezultate. Workerii nu
   primesc niciunul dintre skillurile de rol, rosterul sau instrucțiuni de coordonare:
   primesc sarcina și contextul de la lead, apoi răspund normal. Aplicația
   detectează și livrează rezultatul; workerul nu trebuie să cunoască mecanismul.
   Nu adăugăm drepturi noi și nu limităm editarea fișierelor printr-un rol
   artificial de pane „read only”.
5. O sesiune nouă din cmux/Terminal nu mai primește automat acest skill sau
   roster. Executabilul `cf` poate rămâne pentru administrare umană;
   prezența lui pe PATH nu dovedește disponibilitatea delegării.
6. Păstrăm verificarea autorității în backend: contextul scris în skill sau
   o variabilă de mediu nu înlocuiesc credențialele valide ale aplicației.
   Un apel din afara aplicației primește un refuz scurt, fără lansări sau
   încercări de creare a unui context alternativ.

### Mecanisme native de verificat înainte de trecere

| Harness | Mecanism propus | Ce trebuie demonstrat |
|---|---|---|
| Claude Code | `--add-dir` către un director privat care conține numai `.claude/skills/consensflow-lead/` (sau numai `consensflow-pm/` pentru PM) | Skillul rolului este prezent în acel proces, celălalt absent; ambele absente într-un proces extern nou; funcționează și la resume; politicile native sunt respectate. |
| Pi | `--skill` cu calea privată către documentul Markdown al rolului | Skillul este doar Markdown; extensia separată este descrisă în secțiunea 8; păstrează celelalte skilluri și setările utilizatorului. |
| OpenCode | `skills.paths`, adăugat la configurația pe proces prin `OPENCODE_CONFIG_CONTENT` | Combinare cu valorile existente; același context la serverul nativ și la sesiunea atașată; nimic scris în configurația globală. |
| Codex | Instrucțiuni suplimentare pe sesiune prin override-ul nativ `developer_instructions`, cu referință la documentul privat | Trebuie demonstrată combinarea cu instrucțiunile efective existente, inclusiv profile/proiect, fără înlocuire oarbă. `skills.config` documentează activare/dezactivare, nu dovedește încărcarea unei rădăcini arbitrare. |

Acestea sunt opțiuni native/documentate sau expuse de `--help`, nu probe de
integrare deja trecute. Codex are o verificare obligatorie de compoziție.
Dacă o opțiune nu poate păstra configurația utilizatorului, nu o activăm și
nu revenim în tăcere la instalare globală; revizuim mecanismul în acest plan.

Nu schimbăm `HOME`, `CODEX_HOME`, `CLAUDE_CONFIG_DIR` sau alte rădăcini de
profil ca să simulăm izolarea. Autentificarea, memoria, istoricul, setările,
celelalte skilluri și stilul TUI rămân native. În afara extensiei Pi aprobate, nu instalăm pluginuri, hooks
sau development channels și nu modificăm binarele harness-urilor.
Excepția explicit aprobată este extensia Pi, administrată conform secțiunii 8. Skillul Markdown al rolului nu conține cod de extensie.

### Trecerea la skilluri private și curățarea manuală

**Decizie explicită Gabriel, 2026-09-09:** skillul global vechi se șterge de
către noi, manual. Nu implementăm o funcție de ștergere sau migrare automată
a lui în ConsensFlow. Această decizie înlocuiește propunerea anterioară de
retragere automată a fișierelor deținute prin manifest.

- Mai întâi eliminăm scrierile globale din setup, refresh, actualizarea
  rosterului și auto-heal; acele trasee vor administra conținutul privat.
- Aplicația nouă, instalatorul și updaterul nu șterg, mută sau rescriu skillul
  global vechi. Nu adăugăm un buton, o comandă de curățare sau un scanner de
  migrare pentru el. Cele două skilluri noi sunt administrate automat numai
  în spațiul privat al ConsensFlow.
- La trecerea controlată, noi inventariem și ștergem manual copiile globale
  `consensflow/SKILL.md` pe care le identificăm ca fiind ale instalării vechi.
  Verificăm atunci și căile istorice/symlinkurile, inclusiv descoperirea globală
  Claude/Agents de către OpenCode; nu ștergem ținte ori skilluri străine.
  Aceasta este o operație locală separată, nu comportament al aplicației.
- O schimbare de roster actualizează conținutul privat. La următoarea
  pornire/reluare se livrează versiunea actuală. Un roster vechi din context
  nu poate autoriza un agent șters; backend-ul rezolvă rosterul curent.
- Curățarea manuală are loc după probele pe proces și eliberarea sesiunilor
  protejate, când vechea aplicație nu mai poate regenera copiile globale.
  În acest tur actualizăm numai specul; nu ștergem încă fișiere instalate.

Verificare separată: pornirea, setup-ul, actualizarea aplicației și schimbarea
rosterului lasă identice fișierele globale martor, inclusiv o copie veche
deținută și un symlink. Pe un profil curat nu creează skilluri globale. După
curățarea manuală, aceleași operații nu recreează vechiul skill.

Limita promisiunii: o conversație care a citit deja skillul îl poate avea în
istoric, memorie sau context. Nu îi putem face conținutul necunoscut retroactiv.
După curățarea manuală, garanția privește expunerea automată în porniri noi și autorizarea efectivă a
comenzilor. Un fișier dintr-un proiect despre ConsensFlow poate fi citit normal
de un agent care lucrează la acel proiect; acesta nu primește prin asta drepturi.

Probe negative obligatorii: workerii tuturor harness-urilor, inclusiv Kimi,
și pane-urile shell nu primesc acest skill după curățarea manuală a copiilor
globale. Curățarea noastră acoperă și Kimi, fără să introducă un rol nou de lead.

Fișiere vizate: `src/skill.js`, `src/harnesses.js`, `src/install.js`,
`src/sync.js`, `src/launch.js`, `src/panes.js`, `hosts/lib/runners.js`,
`bin/cf.mjs`, textele de instalare din `src/ui.js` și `README.md`.
Reutilizăm generatorul; administrarea noilor artefacte rămâne privată aplicației.
Nu adăugăm un mecanism de migrare a skillurilor globale.

### Revizia conținutului

[Reviewul skillurilor și al stackului](review-role-skills-and-stack.md)
inventariază problemele cu trimiteri la cod și la copia instalată.
[Draftul lead](draft-skills/consensflow-lead/SKILL.md) și
[draftul PM](draft-skills/consensflow-pm/SKILL.md) sunt texte de discutat, stocate
numai în acest spec. Nu sunt generate, instalate sau oferite proceselor active.

Înlocuim proza veche cu reguli compacte pentru rol și flux. Scoatem timeoutul
de task sincron, diagnosticele de transport, instalarea globală din instrucțiunile
leadului și obligația de a opri lucrul la fiecare răspuns. Nu cerem din nou
aprobarea unei implementări deja autorizate. Separăm explicit: rezultat complet
sosit, referință sosită cu părți, citire cerută și răspuns încă absent.
Textul trebuie să funcționeze și cu roster gol. Păstrăm citirea integrală,
atribuirea și controlul uman asupra politicii, fără a confunda skillul cu
autorizarea backend. Evaluările comportamentale sunt în review; simpla prezență
a unor propoziții în test nu certifică înțelegerea agentului.

## 3. Leadul primește starea sarcinii, nu rolul de depanator al aplicației

Separăm pornirea sarcinii de citirea/livrarea rezultatului. Un adaptor care
nu recunoaște o versiune nu dovedește că modelul nu a pornit. Faptul că un
pane este deschis nu dovedește că a primit sarcina. Observația lui Gabriel
„pare să meargă” se păstrează; nu declarăm acel flux defect fără reproducere.

| Dovezi disponibile | Mesaj pentru lead | Ce nu deducem |
|---|---|---|
| Cererea a fost acceptată de aplicație | Cerere acceptată; pornire în curs | Modelul lucrează deja. |
| Sarcina a fost admisă de harness | Sarcină trimisă; conversație identificată | Rezultatul este gata. |
| Semnal nativ de lucru/încheiere | În lucru / rezultat terminat | Starea tuturor celorlalți agenți. |
| Pane deschis, fără dovadă despre sarcină | Starea sarcinii nu este confirmată | „0 runs” înseamnă că nimic nu s-a executat. |
| Citirea sau livrarea nu poate fi confirmată | Rezultatul nu a fost încă livrat; aplicația indică problema | Taskul nu a pornit sau toate harness-urile similare sunt inutilizabile. |
| Intervenție umană confirmată de mecanismul nativ | Acțiunea concretă necesară | Un presupus dialog de trust, din simpla absență a rezultatului. |

Textele tehnice despre operații, adaptoare și transport rămân disponibile în
diagnosticul aplicației, fără inspectarea versiunilor native, conform secțiunii 5.
Erorile reale nu sunt transformate în succes și nu
sunt ascunse; răspunsul normal către lead explică impactul asupra sarcinii.
Depanarea explicit cerută de utilizator poate consulta diagnosticele.

Skillul devine mai scurt: lansează sarcina, continuă munca independentă,
folosește complet răspunsul primit automat și așteaptă când următorul pas
depinde de un răspuns absent. Leadul nu trebuie să afle modul auto/manual:
pe auto primește răspunsul; pe manual utilizatorul îi cere să-l citească.
Păstrăm citirea tuturor părților unui rezultat deja livrat prin referință.
Nu schimbăm acum formatul rezultatelor mari acceptat anterior.

Corectăm orice fallback care prezintă `0 runs` drept stare de execuție în
fluxul aplicației. Sursa alpha.36 are deja stări de pornire mai precise;
le verificăm și le reutilizăm, nu rescriem mecanisme care funcționează.

Fișiere vizate: `bin/cf.mjs`, `src/skill.js`, proiecțiile de stare din
`src/panes.js`/`src/ui.js`, `app/ui/panes.js` și scenariile din `evals/`.
Nu schimbăm detecția finalizării sau retry-ul transportului fără un test
care reproduce o problemă la acea limită.

## 4. PM: research și planificare, separat de execuție

### Rol și interfață

Confirmat de Gabriel: PM-ul discută produsul cu utilizatorul, face research,
planifică folosind Spec Mint și explică pe înțelesul lui concluziile,
alternativele și consecințele. Are acces la același folder ca leadul și
workerii, dar nu implementează cod și nu distribuie sarcini workerilor.
Poate folosi Claude Code, OpenCode, Pi sau Codex, ales independent de lead.

Propunere pentru prima versiune: un PM opțional per sesiune, creat explicit
din acțiunea „Adaugă PM”. Nu pornim un PM suplimentar la fiecare sesiune.
PM și lead au conversații native diferite, inclusiv când folosesc același
harness. Numele aleator se generează o singură dată și se păstrează la resume;
harness-ul apare separat, fără să țină loc de numele rolului.

```text
pokerbot-pluribus
├─ pm: amber-brook          → fereastră proprie, tot spațiul disponibil
└─ lead: silver-meadow      → session view, fix în stânga, lățime 2×
   ├─ w1 astraeus-bubble-lagoon
   └─ w2 calliope-kelp-meadow
```

În sidebar PM și lead sunt frați, la aceeași indentare; numai workerii sunt
copiii leadului. „Mai sus” descrie responsabilitatea PM-ului pentru planificare,
nu dreptul de a controla automat leadul. PM nu intră în grid, în numărul de
workeri sau în indicatorul rezultatelor workerilor în așteptare.

Propunerea de fereastră: o fereastră ConsensFlow separată, maximă la deschidere,
cu fullscreen nativ disponibil; poate rămâne deschisă alături de sesiune.
Întrebarea despre fereastră separată versus vedere fullscreen în aceeași
fereastră a fost trimisă utilizatorului; această alegere rămâne propusă până
la răspuns. Selectarea unui PM existent îi aduce fereastra în față, fără alt
proces. Închiderea PM-ului închide numai procesul lui și păstrează conversația
pentru reluare; nu oprește leadul/workerii. Suspendarea execuției leadului nu
oprește discuția cu PM-ul. Ștergerea întregii sesiuni și updaterul iau în calcul
și PM-ul; un PM activ nu este omis doar fiindcă nu este în fereastra principală.

### Skillul PM și comunicarea cu leadul

Skill: `consensflow-pm`, disponibil doar procesului PM din aplicație.
Leadul primește `consensflow-lead`; PM-ul nu primește rosterul/comenzile workerilor.
Workerii nu primesc niciunul dintre cele două skilluri. Nu instalăm global.

- PM citește codul pentru a înțelege proiectul. Poate redacta documente și
  specificații în cadrul cererii utilizatorului; aceasta este interpretarea
  propusă a „nu scrie cod”. Nu modifică implementarea, testele, configurațiile,
  dependențele și nu rulează build/deploy în rolul PM.
- Folosește partea de research/forge/revizuire a Spec Mint, cu cerințe și
  criterii verificabile; nu trece singur la implementare și nu activează
  delegarea cerută eventual de un alt skill. Nu instalăm alte skilluri global
  ca efect secundar; disponibilitatea Spec Mint în procesul PM trebuie verificată.
- Explică întâi concluzia și impactul asupra produsului, apoi dovezile și
  compromisurile. Își continuă singur researchul cerut, fără a contacta alți agenți.
- „Planul este bun” nu înseamnă „trimite-l”. Numai cererea utilizatorului de
  transmitere permite un mesaj către lead. PM prezintă ce a trimis și destinația.
- Trimiterea nu autorizează citirea ulterioară. PM nu așteaptă cu polling, nu
  primește automat răspunsuri sau notificări de la lead/workeri și nu are
  mod auto/manual. Citește de la lead doar în urma cererii utilizatorului.
- Când utilizatorul cere citirea, primește rezultatul complet al leadului;
  toate părțile acelui rezultat țin de aceeași cerere. Citirea nu trimite
  un mesaj nou leadului și nu modifică mărcile de citire ale workerilor.

Interfață CLI propusă, încă inexistentă: `cf lead send --message-file <file>`
și `cf lead read [--answer <id>] [--part <k>]`. Prima trimite o singură dată,
cu autorul PM identificat; a doua citește ultima explicație completă a leadului
sau răspunsul indicat. Prima parte returnează ID-ul imuabil al răspunsului;
orice parte ulterioară cere `--answer <același-id> --part <k>`. Un `--part`
fără `--answer` este refuzat, ca un nou răspuns al leadului să nu schimbe
rezultatul citit între părți. Destinația este leadul aceleiași sesiuni, stabilită din
credențială, nu dintr-un folder sau un ID arbitrar. Dacă leadul este închis,
PM explică situația; nu îl pornește și nu programează singur o retrimitere.

Backend-ul permite PM-ului numai aceste operații asupra leadului său și
refuză consult/say/attach/results ale workerilor, administrarea rosterului,
alte sesiuni și schimbarea politicii de livrare. Nu există operație de push
lead → PM. „Nu scrie cod” și cererea explicită în conversație sunt reguli ale
rolului; accesul la același filesystem nu constituie o izolare OS read-only.
Nu prezentăm skillul ca pe o barieră tehnică împotriva oricărei scrieri.

### Implementare vizată și probe

Extindem modelele și traseele existente din `src/tabs.js`, `src/store.js`,
`src/launch.js`, `src/panes.js`, `src/page.js`, `src/ui.js`, `src/skill.js`
și `bin/cf.mjs`; refolosim identitățile native, transportul și readerul.
În `app/ui/sidebar.js`/`panes.js` separăm afișarea PM de grid. Pentru fereastra
separată folosim Tauri existent (`app/src-tauri/src/lib.rs`, `commands.rs` și
capabilitățile ferestrei), cu un singur runtime și rutare output/input după
identitatea PM-ului. Fereastra PM nu primește prin simpla etichetă autoritate
asupra altor terminale. Recomandarea de stack este în secțiunea 7;
păstrarea stilului TUI nativ rămâne o cerință indiferent de framework.

Probe obligatorii: două roluri în același folder și același harness au istorii
distincte; PM absent din grid/pending; nume persistente; mesaj doar la cerere,
exact o singură admitere; niciun push către PM în timp ce leadul/workeri termină;
citire integrală de 25+ KB; interdicții la API pentru rol greșit și altă sesiune;
închidere/reluare PM independentă și updater blocat când PM lucrează. Evaluări
pe fiecare dintre cele patru harness-uri: PM planifică fără cod/delegare și
nu confundă „aprob planul” cu „trimite leadului”.

## 5. Versiuni informative, fără blocare pe numărul versiunii

**Decizie actuală:** detectăm și afișăm versiunea instalată și disponibilitatea
unei versiuni noi în panoul de administrare (secțiunea 9). Aceasta înlocuiește
interdicția totală de citire a versiunilor. Numărul versiunii nu decide lansarea,
binding-ul, citirea sau livrarea. O versiune necunoscută nu este incompatibilitate.

Constatare verificată pe 2026-09-09: aplicația din `/Applications/ConsensFlow.app`
și CLI-ul ei sunt alpha.34; sursa/candidatul local sunt alpha.36. Alpha.34
respinge Claude Code 2.1.266 în `hosts/lib/completion.js::checkedVersion`;
alpha.36 include această versiune. Aceasta este o restricție introdusă de
ConsensFlow în adaptor, nu o cerință a executabilului Claude. Alpha.36 încă
nu satisface noul contract: adăugarea lui 2.1.266 într-o listă nu rezolvă
cerința de eliminare a verificării versiunilor.

Defect confirmat în codul ambelor versiuni: `DeliveryWatcher.readResult`
citește istoricul leadului și cere un cursor verificat înainte să returneze
răspunsul complet al workerului. Astfel un rezultat Codex disponibil este
blocat de versiunea Claude a cititorului. Citirea unei livrări deja persistate
prin `Panes.read` este o rută distinctă; afirmația că toate rutele au exact
același control nu se preia fără verificare.

Contract confirmat pentru versiuni, cu decuplarea citirii planificată:

- Permitem probe limitate de versiune pentru afișare, separate de operațiile
  de sesiune; eroarea/timeout-ul nu blochează lansarea sau citirea rezultatelor.
- Eliminăm listele, comparațiile exacte, intervalele și ramurile bazate pe
  versiunea harness-ului, inclusiv `cli_version` la binding Codex, lista
  Claude peer și selecția finalizatorului Claude. Câmpurile numerice de
  versiune ale înregistrărilor native nu devin un control alternativ de
  compatibilitate pentru aceste patru harness-uri.
- Eliminăm din UI/CLI diagnosticele „unsupported/unverified version”,
  matricea de versiuni acceptate. Afișarea versiunii și a actualizărilor disponibile
  este informativă, conform secțiunii 9.
  Retragem `verified-harnesses.json` și dependența de `verified_harnesses`
  din pregătirea/validarea release-urilor. Metadatele istorice ale unui feed
  mai vechi pot fi ignorate; absența lor nu blochează o actualizare.
- Updaterul continuă să compare **versiunile ConsensFlow** și să verifice
  semnăturile/pachetele sale. Nu inspectează versiunile programelor găzduite.
- Separăm lansarea nativă, identificarea conversației, extragerea rezultatului,
  livrarea automată și confirmarea primirii. Deciziile folosesc operația și
  dovezile efective: identitate, mesaje, structura datelor, evenimentul de
  finalizare, instrumente încă active și confirmarea trimiterii. Un număr
  absent sau schimbat nu este dovadă de eroare.
- Un rezultat deja capturat integral se citește prin API-ul aplicației fără
  verificarea versiunii/readiness/cursorului leadului. Dacă workerul încă poate
  fi citit sigur, rezultatul poate fi capturat atunci. Dacă formatul workerului
  este necunoscut și nu avem captură, spunem că finalizarea nu poate fi confirmată.
- Returnarea textului și confirmarea primirii au stări separate. Lipsa unei
  confirmări native nu blochează textul și nu marchează răspunsul drept primit.
  Păstrăm autorizarea, identitatea conversației, toate părțile și deduplicarea;
  nu reluăm automat o trimitere al cărei rezultat este incert.
- Validăm conținutul și comportamentul necesare operației, fără praguri de
  versiune. Datele incomplete, identitatea greșită sau un mesaj neconfirmat
  rămân cazuri distincte și explicite. Dacă se schimbă efectiv mecanismul
  nativ, adaptăm integrarea; eliminarea numerelor nu dovedește singură că
  orice schimbare viitoare de comportament este deja implementată.
- O limitare a trimiterii automate lasă disponibile terminalul și rezultatele
  deja capturate, cu mesaj în aplicație. Nu reinterpretăm `0 runs` sau o eroare
  de reader drept „agentul nu a pornit”. `cf read` nu este recomandat într-un
  terminal extern, unde nu are autoritatea aplicației.

Fișiere vizate: `hosts/lib/completion.js`, `hosts/lib/session-binding.js`,
`src/delivery-watch.js`, `src/panes.js`, `src/channels.js`,
`src/channels/claude-peer.js`, `src/update-compatibility.js`,
`src/verified-harnesses.json`, `src/ui.js`, `bin/cf.mjs`,
`app/scripts/prepare-update.mjs`, `app/src-tauri/src/commands.rs`,
`app/src-tauri/src/updates.rs`, `app/ui/updates.js` și documentația lor.
Eliminăm traseele/importurile devenite inutile prin această schimbare.

RED obligatoriu: aceleași date native valide dau același rezultat cu număr
vechi, viitor, neobișnuit sau absent; niciun `--version` nu este executat de
produs. Acoperim pornire, binding, read, livrare și updater pentru toate cele
patru harness-uri, inclusiv prin procese reale cu un executabil martor care
înregistrează orice încercare de verificare a versiunii. Completăm testele din
`tests/engine/completion.test.mjs`, `tests/engine/session-binding.test.mjs`,
`tests/channels.test.mjs`, `tests/claude-peer.test.mjs` și înlocuim contractele
de gating din testele de compatibilitate/release/updater. Un rezultat complet
rămâne citibil și fără confirmarea primirii; outputul parțial, sesiunea greșită
și trimiterea cu posibilă admitere rămân protejate prin dovezile lor reale.

## 6. Restart/resume: workerii și rezultatele rămân accesibile

Incident confirmat pe alpha.34, 2026-09-09: după restart/resume, sesiunea
`pokerbot-pluribus` (`t-16`, folder real `poker-bot`) arată doar leadul `p-45`,
generația 6. În `threads.json`, Astraeus și Calliope există încă pentru
`tab:t-16:5`, cu sesiunile native păstrate; nu mai există rândurile lor în
`tabs.json`. Livrarea Astraeus `d-105` este pending, cu motivul
`held for previous lead generation`. Este o pierdere de acces/continuitate
în aplicație, nu o ștergere demonstrată a transcripturilor.

Recuperarea cerută de Gabriel a găsit **ambele reviewuri terminate**, inclusiv
Calliope, contrar diagnosticului leadului. Au fost exportate fără rezumare sau
modificări în `/Users/gabrielvoicu/Projects/ngvoicu/pluribus/reviews/`, la
căile cerute explicit de Gabriel; exporturile temporare din `reports/` au
fost mutate, fără păstrarea unor copii suplimentare:

| Conversație → fișier | Bytes UTF-8 | SHA-256 |
|---|---:|---|
| astraeus-bubble-lagoon → `astraeus-2026-09-09.md` | 24949 | b001720b91cf29411cb4df9137ecc281eeb3b3de9b59c32f730803e56048ccc8 |
| calliope-kelp-meadow → `calliope-2026-09-09.md` | 26168 | 5d8a19978736a5fb6b1c459729b4aafb01a0b4fa032e44673516105e51bdad4f |

Sesiuni native: Codex `01a08665-8e0a-7773-8245-4cc218b3ddd7`, Claude
`33323809-d347-4cdd-87dc-628d8147b6e7`. Readerul sursei alpha.36 a verificat
finalizarea; fișierele exportate au fost comparate byte cu byte cu răspunsurile.
Recuperarea nu a pornit agenți, trimis mesaje, schimbat mărcile de citire sau
modificat starea aplicației. Nu echivalează cu repararea funcției de resume.

Al doilea caz confirmat în același tur: Fortuna `t-41` a fost reluată la
generația 2 și arată doar leadul `p-104`; cele patru conversații persistă pentru
`tab:t-41:1`. `apollo-misty-valley` are un final de 10548 bytes, iar
`gefjon-misty-orchard` unul de 2902 bytes. `zeus-golden-reef` și
`diana-umber-lagoon` nu au final confirmat; ultimele mesaje publice sunt din
14:29:03Z, respectiv 14:30:51Z, despre verificări în curs. `inFlight` în
transcriptul rămas nu dovedește că un proces mai rulează după restart.

În `fortuna-advisor/runtime-archive/reviews/consensflow-recovered-2026-09-09/`
au fost salvate `apollo-2026-09-09.md` și `gefjon-2026-09-09.md` integral;
`zeus-2026-09-09-incomplete.md` și `diana-2026-09-09-incomplete.md` conțin
toate mesajele publice de progres, etichetate ca incomplete, fără reasoning
intern/tool output. Nu certifică implementările sau testele acelor workeri.
Codul, specul și procesele Fortuna nu au fost modificate de recuperare.

Contract propus:

- Identitatea conversației și apartenența la sesiune persistă separat de
  existența procesului/paneului deschis. Închiderea procesului nu șterge
  workerul din navigarea sesiunii; îl afișăm ca închis, cu rezultat disponibil.
- Restartul și reluarea aceleiași sesiuni păstrează numele, ordinea, ID-urile
  native și accesul la rezultate. „Redeschide” reia aceeași conversație;
  „Conversație nouă” creează una nouă. Nu retrimitem taskul inițial.
- O generație nouă a leadului nu ascunde rezultatele generației anterioare.
  Citirea istoricului aparține aceleiași sesiuni persistente; trimiterea în
  editor rămâne legată de procesul/generația curentă. Răspunsurile vechi nu
  sunt împinse automat după restart; cele cu admitere incertă nu sunt reluate.
- Migrarea folosește legătura salvată către `t-16`, nu folderul sau „ultima
  sesiune”. Două sesiuni din același folder nu își însușesc workerii/rezultatele.
- Sidebarul și readerul arată răspunsurile și pentru workeri fără PTY activ;
  pending numără rezultate distincte ale sesiunii, nu workeri, părți sau tentative.
- Workerii întrerupți rămân vizibili cu progresul păstrat și fără rezultat final
  inventat. Un transcript in-flight vechi nu este afișat drept proces activ.
- Redeschiderea istoricului ori citirea unui rezultat nu modifică automat
  specul proiectului și nu produce o nouă execuție a workerului.

Fișiere vizate: `src/store.js`, `src/tabs.js`, `src/panes.js`, `src/page.js`,
`src/delivery-watch.js`, `app/ui/sidebar.js` și `app/ui/panes.js`. Verificăm
separat eliminarea paneului la exit (`Panes.#paneExited`) și recuperarea
rezervărilor (`Store.#restartRecovery`), fără să confundăm procesul cu istoricul.
RED → GREEN pe procese reale: lead + doi workeri terminați, restart, resume,
aceiași doi workeri vizibili, ambele texte integral accesibile, zero redispatch;
include și un worker fără final, crash, exit normal, rezultat de 25+ KB, generație veche, două sesiuni
cu același folder și migrarea formei istorice unde paneurile lipsesc deja.

## 7. Aplicație curată și alegerea stackului

Gabriel a cerut evaluarea unei aplicații refăcute, curate, inclusiv Tauri versus
Electron și folosirea Ghostty. [Reviewul tehnic](review-role-skills-and-stack.md)
separă dovezile din cod de recomandare și include surse primare actuale.

Astăzi: Tauri 2, xterm.js 6 cu FitAddon, PTY în Rust prin `portable-pty` și
runtime Node inclus pentru coordonare. Nu folosim Ghostty. Recomandarea este
păstrarea acestei baze pentru etapa propusă, cu refacerea interfeței și corectarea
modelului de sesiuni/rezultate. Tauri versus Electron nu este același lucru cu
xterm.js versus Ghostty; nu avem măsurători care să certifice un câștig prin migrare.
O rescriere completă sau schimbarea stackului nu este o decizie acceptată încă.

Prin „adaptoare” înțelegem codul inclus în ConsensFlow care conectează aplicația
la fiecare harness: construiește comanda de pornire/reluare, identifică sesiunea
nativă, extrage răspunsul complet și trimite mesajele prin mecanismul disponibil.
Se află în `hosts/lib/runners.js`, `session-binding.js`, `completion.js` și
`src/channels/`. Nu sunt actualizări ale executabilelor Claude/Codex/OpenCode/Pi.
Integrarea Pi folosește `hosts/pi-extension/`, încărcată pe proces; decizia
actuală este păstrarea și administrarea ei conform secțiunii 8. Istoric:
această soluție este respinsă de Gabriel și trebuie eliminată, conform
secțiunii 8. Adaptoarele rămase rulează în ConsensFlow; controalele de versiune
nativă se elimină conform secțiunii 5.

„Curat” are criterii verificabile: două skilluri private pe rol, zero trasee
active de instalare sau ștergere automată a skillului global vechi, zero
blocări bazate pe versiunea nativă, aceeași citire
integrală pentru manual/automat, istoric independent de pane și eliminarea
codului făcut inutil de aceste schimbări. Documentația operațională se aliniază
produsului; TDD Log păstrează istoricul fără a-l prezenta ca instrucțiune actuală.
Nu înseamnă ștergerea datelor utilizatorului sau modificarea profilurilor native.

Propunere vizuală: ierarhie clară sesiune/lead/PM/workeri, controale consecvente,
spațiu pentru terminale și pentru stările utile utilizatorului; fără diagnostice
interne repetate pe carduri. Nu recolorăm uniform outputul harness-urilor.
Verificăm culori, bold/dim, selecție, IME, input și resize în procese native,
pe lângă probele geometrice. Dacă un blocaj rămâne în renderer, măsurăm înainte
de a alege altul, conform comparației 1/5/20 pane-uri din review.

## 8. Extensia Pi administrată de ConsensFlow

**Decizie actuală, confirmată de Gabriel:** înlocuiește interdicția anterioară
a extensiei Pi. Dacă detectăm executabilul Pi instalat, încercăm automat să
instalăm/pregătim extensia ConsensFlow din bundle-ul aplicației. Dacă Pi nu este
instalat, nu creăm extensia și nu instalăm Pi în locul utilizatorului.

Extensia este o componentă distinctă de skillurile Markdown. O păstrăm în
spațiul privat ConsensFlow și o încărcăm numai în procesele Pi pornite de app;
nu schimbăm configurația globală Pi sau extensiile utilizatorului. Operația
este idempotentă, atomică și folosește componenta aceleiași versiuni ConsensFlow.
Pornirea aplicației și o verificare explicită pot declanșa pregătirea dacă Pi
este prezent; eșecul nu produce o buclă de retry. Butonul „Reîncearcă instalarea”
permite o nouă încercare. Sesiunile active nu sunt repornite sau modificate.
O componentă actualizată se aplică proceselor noi; pentru cele active afișăm
„Disponibilă pentru următoarea pornire”, fără a le declara automat funcționale.

În dreptul Pi: „Se instalează”, „Instalată, neverificată”, „Funcțională”, sau
**roșu: „Extensie lipsă / instalare eșuată”**, cu motiv și retry. Fișierul prezent
nu dovedește funcționarea: verificăm handshake-ul integrării în contextul unui
proces app-managed; fără un proces verificabil afișăm „Neverificată”.

Acceptare: Pi absent nu produce instalare; Pi prezent declanșează instalarea;
repetarea nu dublează extensia; erorile de permisiune și instalarea întreruptă
sunt raportate și recuperabile. Probe native pentru lead/worker/PM, rezultat
integral, finalizare observată, draft păstrat, trimitere unică, două sesiuni și
restart. PM nu primește push chiar dacă extensia este disponibilă. Păstrăm TUI-ul
original. Testele folosesc profiluri izolate; instanța protejată nu este atinsă.


## Ordinea implementării, după acordul asupra planului

| Etapă | RED | Implementare minimă | Dovadă pentru închidere |
|---|---|---|---|
| 0. Rezultate și continuitate | Testele secțiunii 5 pentru zero verificări de versiune; `tests/delivery-watch.test.mjs`, `tests/integration/results-reader.test.mjs`, `tests/store.test.mjs`, `tests/tabs.test.mjs` și `app/tests/page.spec.mjs` pentru restart, worker fără pane și generație veche. | Eliminăm toate controalele de versiune nativă, decuplăm citirea de confirmare și păstrăm accesul după restart, conform secțiunilor 5–6. | Zero probe/comparații de versiune; reviewuri accesibile în produs fără restart de worker ori recuperare manuală din storage. |
| 0b. Extensia Pi | Probe native și instalare condiționată conform secțiunii 8. | Extensie din bundle, privată, pregătită automat numai când Pi este detectat. | Rezultat complet automat, draft păstrat, status verificabil, fără modificarea sesiunilor active. |
| A. Layout | Actualizăm contractele geometrice din `tests/layout.test.mjs` și adăugăm probe Xterm în `app/tests/page.spec.mjs`. | Lead fix și zona workerilor cu două rânduri/scroll orizontal. | Raport, poziție fixă și acces la toate pane-urile; outputul din afara ecranului nu se oprește. |
| B. Încărcare numai la lead | Probe izolate de pornire/resume pentru cele patru harness-uri de lead, plus un skill martor al utilizatorului; probe negative pentru workeri, shell și procese externe. | `consensflow-lead` privat, legat de build, încărcat numai în procesul leadului. | Disponibil pentru leadurile Claude/OpenCode/Pi/Codex; absent la workeri și procese externe noi; configurația și skillul martor se păstrează. |
| C. Administrare privată | Teste în `tests/install.test.mjs` și `tests/launch.test.mjs`: profil curat, copie globală veche deținută, fișier modificat, symlink, pornire/setup/upgrade/roster și autoritate absentă. | Oprim instalarea globală și administrăm numai conținutul privat; fără ștergere/migrare automată a skillului vechi; actualizăm UI/README. | Fișierele globale existente rămân identice, cele absente nu sunt create; după curățarea noastră manuală nu se regenerează skillul vechi; contextul invalid nu poate delega. |
| D. Mesaje și skill | `tests/cf-standalone.test.mjs`, `tests/skill.test.mjs` și scenarii comportamentale pentru dovezi insuficiente, rezultat automat și cerere manuală. | Mesaje despre impactul asupra sarcinii și instrucțiuni fără diagnostice presupuse. | `0 runs`/idle nu devin o cauză inventată; rezultatul automat este folosit fără o cerere suplimentară. |
| E. PM independent | Teste rol/istoric/mesaje în `tests/tabs.test.mjs`, `tests/launch.test.mjs`, `tests/skill.test.mjs`, teste HTTP reale și ferestre în `app/tests/page.spec.mjs`/`app/src-tauri/tests/headless.rs`. | Rol PM și skill propriu, nume pentru PM/lead, fereastră separată, numai send/read către lead la cerere. | PM nu intră în grid, nu delegă și nu primește push; probe pe cele patru harness-uri și lifecycle/updater conform secțiunii 4. |
| F. Candidat integrat | Teste de regresie relevante și probe de aplicație în copie izolată. | Ambalare într-o versiune nouă, cu aceleași surse pentru app/cf/skill/adaptoare. | Layout, roluri și izolare în app reală; restart cu istoricul workerilor; livrare automată observată la lead, drafturi și updater păstrate. |

Fiecare etapă este un ciclu RED → GREEN → revizuire. O probă deja verde
înregistrează un comportament existent; nu inventăm o modificare necesară.
Adăugăm taskurile numerotate în SPEC numai când acest draft este acceptat.
Testele vor folosi modulele reale și procese la limitele de autoritate;
fixtures la limitele externe, nu o aplicație înlocuită de mockuri.

În etapa de implementare nu delegăm munca unor agenți ConsensFlow. Probele
funcționale ale produsului, inclusiv porniri native controlate, sunt distincte
de delegarea implementării. În acest tur de planificare nu au fost pornite.
Instalarea pe mașina utilizatorului și publicarea rămân operații separate,
după verificarea candidatului și închiderea sesiunilor protejate de utilizator.
La această trecere ștergem noi manual skillul global vechi, separat de
instalator/updater, înaintea verificărilor de izolare pe această mașină.

Acceptarea livrării automate cere rezultat complet observat în conversația
leadului și confirmare verificabilă, pentru fiecare harness de lead oferit.
Eticheta „Automatic”, un pending count ori un test UI verde nu sunt dovada
livrării. Verificăm și workerii din afara ecranului, două sesiuni simultane,
draft uman prezent și PM fără niciun mesaj primit automat.

## Dovezi folosite pentru acest plan

- Cod local citit: instalare globală în `src/install.js` și `src/harnesses.js`;
  regenerare în `src/sync.js`; autorizare în `src/launch.js`; layoutul actual
  în `src/layout.js` și `app/ui/panes.js`; statusurile în `bin/cf.mjs`;
  excluderea Kimi din leaduri în `src/tabs.js` și `app/ui/index.html`.
- [Claude Code: skilluri din directoare suplimentare](https://code.claude.com/docs/en/skills#load-skills-from-a-directory-outside-the-project).
  Documentația descrie încărcarea pentru sesiunea lansată cu `--add-dir`.
- [OpenCode: configurație pe proces](https://opencode.ai/docs/config/) și
  [schema oficială `skills.paths`](https://opencode.ai/config.json).
- [Tauri: ferestre și fullscreen](https://v2.tauri.app/reference/javascript/api/namespacewebviewwindow/)
  și [capabilități pe fereastră](https://v2.tauri.app/security/capabilities/).
  Comenzile custom nu devin limitate la PM doar prin redenumirea ferestrei;
  rutarea și verificările din backend trebuie testate explicit.
- [OpenAI: referința configurației Codex](https://learn.chatgpt.com/docs/config-file/config-reference)
  descrie `developer_instructions` și sensul `skills.config`;
  [descoperirea skillurilor](https://learn.chatgpt.com/docs/build-skills#where-codex-loads-local-skills)
  explică de ce o instalare în directoare globale sau în proiect nu izolează
  capabilitățile după aplicația care a pornit procesul.
- `claude --help`, `codex --help`, `pi --help`, `kimi --help`, `opencode --help`
  citite local pe 2026-09-09; Pi expune `--skill`. Cercetarea inițială a
  opțiunii Kimi nu intră în soluție: Kimi nu este lead. Nu s-au pornit conversații.
- Nu există un `AGENTS.md` local în proiect sau directoarele părinte verificate;
  instrucțiunile globale furnizate de Gabriel se aplică. Advisor/Context7 nu
  sunt disponibile în această sesiune; nu am folosit agenți ca substitut.

## Stare la încheierea planificării (istoric)

Draft extins cu PM, eliminarea verificării versiunilor native și restart; implementare
neîncepută, teste de aplicație nerulate. Confirmat: lead fix; coordonare doar
la leadurile Claude/OpenCode/Pi/Codex prin `consensflow-lead`; PM cu
`consensflow-pm`, în afara gridului,
cu comunicare explicită către/de la lead. Confirmat ulterior: nicio verificare
a versiunii Claude/OpenCode/Pi/Codex, pe niciun traseu al produsului.
Istoric, supersedat de secțiunile 8–9: interdicția extensiei Pi și căutarea
unui mecanism înlocuitor fără extensie
și demonstrat; testele istorice cu extensia nu închid această cerință.
Propuse încă: fereastră PM separată,
un PM opțional per sesiune, detaliile numerotării și mecanismul Codex.
Reviewul skillului este scris, cu două drafturi și scenarii de acceptare.
Ștergerea skillului global vechi este o operație manuală a noastră, fără
funcție de ștergere/migrare în aplicație. Nu a fost executată în acest tur.
Păstrarea Tauri/xterm.js este recomandarea tehnică; rescrierea integrală și
schimbarea frameworkului nu au fost autorizate.
Ambele reviewuri Pluribus recuperate integral în `pluribus/reviews/`; pentru
Fortuna au fost recuperate două finale și progresul incomplet al altor doi.
Următorul pas este discutarea
draftului; nicio copie globală de skill, aplicație instalată sau sesiune activă
nu a fost modificată. Nu s-a executat restart în acest tur.


## 9. Panoul Harnesses: instalare, versiuni, integrare și skilluri livrate cu app

Această decizie supersedează orice text istoric din document care interzice
extensia Pi sau orice citire a versiunilor. Implementarea acestor controale
rămâne de făcut; niciun status verde nu este revendicat prin această modificare.

Fiecare harness cunoscut (Claude Code, Codex, OpenCode, Pi și Kimi) are un rând,
inclusiv dacă nu este instalat. Kimi rămâne doar worker, nu lead/PM.

| Câmp | Comportament |
|---|---|
| Instalare | Detectat / neinstalat, calea executabilului efectiv folosit. |
| Versiune locală | Valoare verificată sau „Nu s-a putut verifica”, cu motiv și dată. |
| Actualizări | Versiunea oficială mai nouă și link/instrucțiuni, „La zi”, „Neverificat” sau „Verificare eșuată”. Offline nu înseamnă la zi. |
| Integrare | „Funcțională” numai pe dovezi de capabilități; „Neverificată”, „Incompletă” sau „Eroare” cu explicație. Versiunea singură nu dă OK/eroare. |
| Acțiuni | „Verifică din nou” per rând și „Verifică toate”; instrucțiuni de instalare/actualizare după caz. Pi are în plus starea extensiei și retry. |

Detectarea și versiunea locală se verifică la deschiderea panoului, cu timeout
și fără cerere către model. Verificarea unei versiuni mai noi folosește sursa
oficială a distribuției detectate și arată momentul verificării; rezultatele se
cache-uiesc, iar retry explicit le reîmprospătează. Un număr necomparabil apare
ca necunoscut, nu ca incompatibil. Nu actualizăm automat executabilele harness.
Nu executăm comenzi provenite din metadatele remote. Verificarea capabilităților
nu trimite prompturi în sesiuni active și nu consumă tokenuri de model.

Eliminăm textul fals „1 skills in each of 5 harnesses” și „consults via the
generated skill” pentru toți. Afișăm rolurile reale: skill lead numai la lead,
skill PM numai la PM, niciun skill de coordonare la workeri/procese externe.

Fiecare release ConsensFlow livrează propriile `consensflow-lead` și
`consensflow-pm`, cf, adaptoare și extensia Pi împreună. Eliminăm „Update skills”
și acțiunile publice separate de instalare/actualizare a skillurilor; retragem
comenzile CLI echivalente cu mesaj explicativ, fără modificări. Rosterul poate
actualiza datele private ale agenților, nu descarcă altă versiune de instrucțiuni.
Afișăm „Skilluri incluse în ConsensFlow <versiune>”. Aplicația nu șterge skillul
global vechi; curățarea lui rămâne manuală. La update, componentele noi nu sunt
încărcate forțat în procesele deja active.

Acceptare: matrice prezent/absent, versiune validă/necunoscută/eroare/timeout,
update disponibil/la zi/offline, probe de integrare reușite/eșuate, Pi prezent
cu/fără extensie, instalare automată/retry/idempotentă, fără mutarea profilurilor
utilizatorului și fără restartul sesiunilor active. Testele de UI verifică toate
stările și acțiunile, nu doar culoarea. Regresii negative: nicio allowlist de
versiuni pe read/run/send și nicio instalare globală sau „Update skills”.
