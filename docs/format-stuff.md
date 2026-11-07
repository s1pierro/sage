# Format `.stuff` — version 1

Le format des **entités** de S.A.G.E. : tout ce qu'il faut pour charger une chose dans
le monde, qu'il s'agisse d'un personnage, d'un véhicule, d'un bâtiment, d'un arbre ou
d'un banc. Les mêmes fichiers rangent aussi le monde lui-même, par régions.

C'est **le format du moteur**, et le seul qu'il lise en jeu. S.A.G.E. le produit
lui-même à partir de fichiers standards : glTF/GLB, `.gda` de Rustcraft, images PNG ou
JPEG… Il ne porte donc **aucune donnée d'édition de maillage**, en particulier pas de
pile de modifieurs : seulement des maillages figés, prêts à dessiner. Les éditeurs
gardent leurs propres fichiers, et l'on refait le `.stuff` à partir d'eux.

Rien n'est encore codé : ce document fixe le format avant le code. Les décisions qui
l'ont façonné sont rappelées à la fin.

## Principes

- **Une entité = une définition + des instances.** La définition dit ce qu'est la chose
  (maillages, armature, niveaux de détail, collision…). L'instance la pose dans le monde
  (où, sous quel identifiant, d'où elle vient, ce qui a été retouché). Mille bancs
  partagent une définition. Un bâtiment unique a la sienne, rangée à côté de son
  instance.
- **Des niveaux de détail du plus loin au plus près.** Le niveau le plus bas peut être
  **procédural** : quelques paramètres (un contour et un toit, une essence d'arbre)
  dont le moteur tire le maillage. Remplacer un bâtiment généré, c'est lui ajouter des
  niveaux plus détaillés. Le procédural reste pour le lointain et sert de secours.
- **Les données faites à la main priment.** Une instance dit d'où elle vient
  (`provenance`) et si elle a été retouchée (`retouche`). Régénérer depuis une source
  ne touche jamais une instance retouchée ou ajoutée à la main sans qu'on le demande.
- **Les identifiants partout.** Toute référence passe par un identifiant : définition,
  partie, os, pose, animation, masque, mélange, point, niveau, instance. Les noms ne
  servent qu'à l'affichage.

## Conventions

Celles du `.gda` (voir `rustcraft/docs/format-gda.md`), reprises telles quelles :

- **Repère** : Y vers le haut, main droite. Un personnage regarde vers +Z, et sa gauche
  est en +X.
- **Unités** : le mètre et la seconde.
- **Quaternions** : `[x, y, z, w]`, normalisés à la lecture (nul ou illisible : l'identité).
- **Angles** : degrés pour les limites des os, quaternions partout ailleurs.
- **Couleurs** : `"#rrggbb"`.
- **Identifiants** : chaînes opaques, uniques parmi les leurs dans leur portée (voir
  chaque section).
- **Clés inconnues** : ignorées à la lecture, gardées et réécrites telles quelles. Les
  tampons qu'elles désignent sont gardés aussi, à leur rang : un outil qui réécrit un
  fichier ne renumérote jamais un tampon qu'il ne comprend pas.

## Le conteneur

Un `.stuff` est un fichier binaire, à la manière d'un `.glb` : un en-tête, un
**manifeste** JSON et des **tampons** binaires.

| Octets | Contenu |
|---|---|
| 0–7 | `STUFF\0\0\0` |
| 8–11 | version du conteneur, u32 LE (1) |
| 12–15 | longueur totale du fichier en octets, u32 LE : un fichier tronqué se détecte |
| 16–19 | longueur du manifeste en octets, u32 LE |
| 20… | manifeste, JSON UTF-8, complété d'espaces pour que les tampons commencent sur un multiple de 8 |
| … | tampons, mis bout à bout, chacun aligné sur 8 octets |

Le manifeste porte la structure, et les tampons les gros volumes : sommets, clés
d'animation, poids, images, relief, tables d'instances. Un tampon se désigne par
`{ "tampon": k, "debut": o, "taille": n, "type": "f32" | "u32" | "u16" | "i16" | "u8" }`,
où `k` est son rang dans la liste `tampons` de la racine, `debut` un décalage **en
octets** dans ce tampon (multiple de la taille du type), et `taille` un **nombre
d'éléments** du type. Une référence ne désigne qu'un seul type : des données de types
différents prennent chacune la leur.

L'outil `stuff` (à écrire) affiche le manifeste et le résumé des tampons, et peut
convertir un fichier en JSON lisible (tampons en base64) et retour.

## Racine du manifeste

```json
{
  "format": "stuff",
  "version": 1,
  "application": "S.A.G.E. 0.26",
  "id": "<id du fichier>",
  "genre": "entite" | "catalogue" | "region",
  "tampons": [{ "taille": 123456 }, …],
  "definitions": [ … ],
  "instances": [ … ],
  "tableaux": [ … ],
  "region": { … }
}
```

| Clé | Rôle |
|---|---|
| `format`, `version` | Obligatoires. Un fichier plus récent que le lecteur est refusé. |
| `id` | Identifiant du fichier. Une définition d'un autre fichier se désigne par `<id du fichier>/<id de définition>`. |
| `genre` | `entite` : une définition, avec ou sans instance, par exemple un personnage. `catalogue` : des définitions à réutiliser, par exemple le mobilier ou les essences d'arbres. `region` : un morceau du monde. |
| `definitions` | Les définitions du fichier. |
| `instances` | Les instances détaillées, une par une. |
| `tableaux` | Les instances en masse, en colonnes binaires (les arbres). |
| `region` | Le terrain d'une région. |

## Définition

```json
{
  "id": "d1",
  "nom": "Maison de la rue Eau-de-Robec",
  "genre": "batiment",
  "etiquettes": ["colombages"],
  "parties": [ … ],
  "materiaux": { … },
  "textures": { … },
  "armature": { … },
  "niveaux": [ … ],
  "collision": { … },
  "points": [ … ],
  "physique": { … },
  "proprietes": { … }
}
```

- **`genre`** : `batiment`, `vegetal`, `mobilier`, `vehicule`, `personnage` ou `decor`.
  Il oriente le moteur (streaming, physique, comportements), mais aucune section n'est
  réservée à un genre.
- **`etiquettes`** : des mots libres, pour la recherche dans le catalogue.
- **`proprietes`** : des paires clé–valeur libres pour le jeu (prix, poids porté…).

### Parties (maillages)

Une **partie** est un maillage avec son matériau, sa place dans la définition et,
s'il y a lieu, sa liaison à l'armature. Elle est rangée **à la manière du glTF**, prête
à être envoyée telle quelle au processeur graphique : la conversion fait le travail une
fois pour toutes, et le chargement n'en fait aucun.

```json
{
  "id": "p1", "nom": "corps",
  "position": [0, 0, 0], "rotation": [0, 0, 0, 1], "echelle": [1, 1, 1],
  "materiau": "peau",
  "nombre": 4000,
  "positions": { "tampon": 0, "debut": 0, "taille": 12000, "type": "f32" },
  "normales": { "tampon": 0, "debut": 48000, "taille": 12000, "type": "f32" },
  "uv": { "tampon": 0, "debut": 96000, "taille": 8000, "type": "f32" },
  "triangles": { "tampon": 0, "debut": 128000, "taille": 18000, "type": "u32" },
  "os": { "tampon": 0, "debut": 200000, "taille": 16000, "type": "u8" },
  "poids": { "tampon": 0, "debut": 216000, "taille": 16000, "type": "f32" }
}
```

- **`nombre`** : le nombre de sommets. Chaque attribut en donne une valeur par sommet.
- **Les sommets sont déjà séparés** aux coutures d'UV et aux arêtes vives : un point du
  modèle qui porte deux UV ou deux normales y devient deux sommets.
- **`positions`**, **`normales`** : trois f32 par sommet. Les normales sont unitaires.
- **`uv`** : deux f32 par sommet. Facultatif.
- **`triangles`** : trois indices de sommet par triangle, la face avant dans le sens
  trigonométrique. Les polygones des fichiers source sont triangulés à la conversion.
- **`os`** et **`poids`** : pour une partie liée à l'armature, quatre os et quatre poids
  par sommet, comme en glTF. `os` donne le rang de l'os dans `liaison`, et les poids
  d'un sommet font 1 au total.
- **`liaison`** : la liste des identifiants des os auxquels renvoient les rangs de `os`,
  par exemple `"liaison": ["bassin", "cuisse.G", …]`. Une partie liée en compte au plus
  256 ; au-delà, elle se coupe en plusieurs parties.
- Les données d'édition des fichiers source (coutures, fils, pile de modifieurs du
  `.gda`) ne passent pas : la conversion applique les modifieurs et n'écrit que le
  résultat.

### Matériaux et textures

Ceux du `.gda` : `materiaux` par identifiant (et non plus par nom), avec les mêmes champs
(couleur, spéculaire, brillance, opacité, `deux_cotes`, `texture`…). `textures` par
identifiant : `{ "nom": "peau.png", "image": <tampon u8> }`, en PNG ou JPEG dans un
tampon plutôt qu'en base64.

### Armature

Celle du `.gda`, sans changement de sens :

- `os` : `id`, `nom`, `parent`, `position`, `rotation`, `longueur`, `contraintes`
  (rotation et translation : libre, bloqué, limité) ;
- `pose`, `poses` (catalogue), `ancre` ;
- `animations` : `id`, `duree`, `boucle`, `mode` (normal ou additif), `reference`,
  `vitesse`, `marqueurs`, et des `cles`, **toujours en tampons binaires** :
  `{ "temps": <f32, une valeur par clé>, "os": [<id os>…], "r": <f32 × 4 par os et par clé>, "t": <f32 × 3 par os et par clé> }`.
  Les os d'`os` sont ceux que l'animation anime ; `t` est facultatif (pas de
  translation). Pour lire une animation, l'outil `stuff` en donne la vue JSON ;
- `masques` et `melanges` : les couches, mêmes règles d'évaluation.

### Niveaux de détail

```json
"niveaux": [
  { "id": "n0", "jusqua": 3000, "procedural": { "generateur": "batiment", "params": { … } } },
  { "id": "n1", "jusqua": 400, "parties": ["p1", "p2"] },
  { "id": "n2", "jusqua": 60, "parties": ["p3", "p4", "p5"] }
]
```

- Les niveaux vont **du plus loin au plus près**. `jusqua` est la distance en mètres
  jusqu'à laquelle le niveau se montre. Le moteur prend le plus détaillé dont la
  distance couvre l'instance. Au-delà du `jusqua` du premier niveau, l'entité n'est plus
  dessinée : seule la **silhouette** de la région la représente.
- Un niveau est soit **procédural** (un générateur et ses paramètres, voir plus bas),
  soit une liste de **parties**.
- Une définition sans `niveaux` montre toutes ses parties à toute distance.
- **Silhouette** : le moteur la tire du premier niveau, et la fusionne par région pour
  le très lointain. Elle ne s'écrit pas.

### Collision

```json
"collision": { "formes": [
  { "boite": { "centre": [0, 1, 0], "demi": [2, 1, 3], "rotation": [0, 0, 0, 1] } },
  { "capsule": { "a": [0, 0.3, 0], "b": [0, 1.5, 0], "rayon": 0.3 } },
  { "triangles": "p9" }
] }
```

ou `{ "niveau": "n0" }` pour reprendre la géométrie d'un niveau, procédural compris.
Elle peut aussi être attachée à un os (`"os": <id>`) pour suivre l'armature. Sans
`collision`, rien n'arrête.

### Points

Des ancres nommées par identifiant, pour accrocher, s'asseoir, ouvrir ou tenir.

```json
{ "id": "porte1", "genre": "porte", "position": [..], "rotation": [..], "os": null }
```

Les genres sont libres, avec quelques genres connus : `porte`, `siege`, `roue`,
`main`, `prise`, `camera`.

### Physique

- `masse` (kg) et `centre` (centre de masse).
- Pour un véhicule :
  `"roues": [{ "point": "roue.AVG", "rayon": 0.32, "suspension": 0.2, "motrice": true, "directrice": true }]`.
- Pour un personnage : la capsule de déplacement, `pas` (la hauteur de marche franchie)
  et `pente` (la pente maximale).
- Absente : l'entité est statique.

## Générateurs procéduraux

Un niveau procédural donne un générateur et ses paramètres. Le moteur en tire le
maillage et, s'il y a lieu, la collision. Version 1 :

- **`batiment`** :
  - `contour`, une liste `[x, z]` dans le repère de la définition ;
  - `pied`, `egout`, `faitage` (hauteurs, en mètres) ;
  - `toit` : `plat`, `deux_pans`, `croupe` ou `appentis`, et `axe` (radians) ;
  - `murs` et `couverture` (couleurs).

  C'est le bâtiment actuel du lieu réel (`src/toits.rs`).
- **`arbre`** : `essence` (`feuillu`, `conifere`), `hauteur`, `teinte` (0–1) et `cap`.
  C'est l'arbre actuel (`src/vegetation.rs`).
- **`pont`** : la ligne (x, z, hauteur du tablier), `largeur` et `teinte`.

D'autres viendront (mobilier simple, murets, haies). Un générateur inconnu ne se
montre pas, et ses paramètres sont gardés.

## Instance

```json
{
  "id": "i-5f3a-12",
  "definition": "d1",
  "procedural": null,
  "tuile": [52, -31],
  "position": [12.5, 31.2, -4.0],
  "rotation": [0, 0, 0, 1],
  "echelle": 1,
  "provenance": "osm/way/123456789",
  "retouche": false,
  "surcharges": { "procedural": { "faitage": 14.2 }, "materiaux": { … } },
  "etat": { … }
}
```

- **`definition`** : un identifiant local au fichier, ou `<id du fichier>/<id>` vers un
  catalogue.
- **`procedural`** : à la place de `definition`, une instance peut porter directement un
  générateur et ses paramètres, sous la même forme qu'un niveau procédural :
  `{ "generateur": "batiment", "params": { … } }`. C'est le cas des bâtiments générés,
  qui n'ont pas de définition propre. Un tel bâtiment qu'on enrichit (niveaux ajoutés à
  la main) reçoit sa propre définition, dont le premier niveau reprend ces paramètres,
  et passe `retouche: true`.
- **`tuile`** : les indices (i, j) de la tuile de 64 m où se trouve l'instance, qui peuvent
  être négatifs. Une instance qui se déplace (un personnage, un véhicule en route) en
  change ; une instance de région est rangée sous la sienne.
- **`position`** : dans le repère de cette tuile (de 0 à 64 m sur x et z), la hauteur
  absolue.
- **`provenance`** :
  - `osm/<type>/<id>` : venu d'OSM ;
  - `lidar` : mesuré ;
  - `genere/<générateur>/<clé>` : par exemple `genere/arbre/1520/-340`, une cellule ;
  - `main` : posé à la main.
  - `+` les combine : `osm/way/123+lidar`.
- **`retouche`** : `true` dès qu'une main y a touché (déplacée, modifiée, niveau
  ajouté…). Une régénération la laisse alors en paix.
- **`surcharges`** : ce qui diffère de la définition pour cette instance seulement :
  des paramètres du générateur ou des matériaux. Sans objet pour une instance
  `procedural`, qui porte déjà ses propres paramètres.
- **`etat`** : pour ce qui bouge, l'état sauvegardé (pose, animation en cours,
  véhicule ouvert…). Absent pour le statique.
- **Une instance masquée** (un bâtiment d'OSM qu'on a démoli) garde sa place :
  `"masquee": true`. Ainsi, régénérer ne la fait pas revenir.

## Tableaux (instances en masse)

Pour les millions d'arbres, une instance JSON par arbre serait trop lourde. Un
**tableau** range des instances d'une même sorte en colonnes binaires.

```json
{
  "id": "arbres",
  "definition": "vegetal/arbre",
  "nombre": 16384,
  "colonnes": {
    "id": <u32>, "tuile": <i16 × 2>, "x": <f32>, "y": <f32>, "z": <f32>, "cap": <f32>,
    "echelle": <f32>, "params": { "essence": <u8>, "hauteur": <f32>, "teinte": <u8> },
    "provenance": <u8 : 0 généré, 1 main>, "retouche": <u8>, "masquee": <u8>
  }
}
```

- Les identifiants d'un tableau sont `<id du tableau>/<n>`.
- Une instance de tableau qu'on retouche beaucoup (qu'on remplace par un autre modèle)
  peut en sortir pour devenir une instance détaillée. Sa ligne est alors masquée.

## Région

Un fichier `genre: "region"` couvre 16 × 16 tuiles, soit 1 024 m. Il se nomme
`regions/<ri>_<rj>.stuff`, et la tuile (i, j) est dans la région (⌊i/16⌋, ⌊j/16⌋).

```json
"region": {
  "ri": 3, "rj": -2,
  "relief": { "pas": 2, "cote": 513, "base": -8.3, "echelle": 0.01, "hauteurs": <u16> },
  "eau": { "niveau": -0.3, "masque": <bits> },
  "occupation": { "pas": 8, "cote": 129, "classes": <u8> },
  "peinture": { … },
  "tuiles": { "index": <u32 × 2 par tuile> }
}
```

- **`relief`** : une grille partagée par les 256 tuiles, bords compris (les tuiles
  voisines lisent les mêmes points), soit 513 × 513 points à 2 m. La hauteur vaut
  `base + v × echelle`. C'est ce que retouche l'éditeur.
- **`eau`** : le niveau et un bit par point du relief.
- **`occupation`** : la classe du sol (bois, pré, champ, habité, zone) sur la grille de
  8 m. C'est elle qui teinte le sol, et que l'éditeur peindra.
- **`peinture`** : les couleurs du sol peintes à la main, tuile par tuile, seulement là
  où il y en a.
- **`tuiles.index`** : pour chaque tuile, le rang de sa première ligne dans chaque
  tableau (les lignes sont triées par tuile). La **région est l'unité de chargement** :
  le moteur lit son manifeste en entier (un kilomètre carré, quelques milliers
  d'instances), et l'index sert à trouver directement les données binaires d'une tuile,
  par exemple pour ne construire que les tuiles proches.
- **Silhouette** : le maillage lointain d'une région (relief simplifié, silhouettes des
  bâtiments, canopée) peut y être rangé une fois calculé, sous `silhouette`, pour ne pas
  le refaire à chaque lancement.

## Le monde

Un monde est un dossier :

```
monde.json            la fiche (id, nom, joueur…), comme aujourd'hui, et le registre des catalogues
regions/<ri>_<rj>.stuff
catalogue/*.stuff     les définitions partagées (essences, mobilier, véhicules…)
entites/*.stuff       ce qui n'est pas lié au terrain (personnages, véhicules en route)
sources.json          d'où vient le monde (lieu.reel, MNH…), pour les régénérations
```

**Le registre des catalogues** : dans `monde.json`, `"catalogues": { "<id du fichier>":
"catalogue/mobilier.stuff", … }`. Une définition `<id du fichier>/<id>` se trouve par
lui, sans recherche ; un catalogue manquant se signale au chargement du monde. La
matérialisation l'écrit en parcourant `catalogue/`.

**La matérialisation** (`examples/lieu_reel.rs`, puis un outil dédié) écrit toutes les
régions à partir du `.reel` et des sources. Le jeu ne lit plus que le dossier du monde.
**Régénérer un fragment** relit les sources pour une zone, et ne touche que les
instances ni retouchées ni ajoutées à la main.

## Décisions prises

- **Les modifieurs** ne figurent pas dans le `.stuff` : il ne porte que des maillages
  figés.
- **La production des `.stuff`** revient à S.A.G.E., qui les tire de fichiers standards
  (glTF/GLB, `.gda`, images). Le moteur ne lit en jeu que du `.stuff`.
- **La géométrie** est rangée à la manière du glTF (sommets séparés, attributs par
  sommet, triangles, quatre os par sommet) : prête pour le processeur graphique.
- **Les clés d'animation** sont toujours en tampons binaires ; l'outil `stuff` en donne
  la vue JSON.
- **Les bâtiments générés** sont des instances qui portent leurs paramètres
  procéduraux, sans définition propre, jusqu'à ce qu'on les enrichisse.
- **Les catalogues** se trouvent par un registre dans `monde.json`.
