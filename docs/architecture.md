# Architecture de S.A.G.E.

Le socle technique se construit en quatre chantiers, dans cet ordre. Chacun s'appuie
sur le précédent.

| Chantier | Version | Ce qu'il apporte |
|---|---|---|
| 1. Architecture ✓ | 0.2 | Monde d'entités, pas de temps fixe, ressources, scène à dessiner |
| 2. Monde en tuiles ✓ | 0.3 | Terrain découpé en tuiles chargées autour du joueur, coordonnées sans perte au loin |
| 3. Physique ✓ | 0.4 | Capsule, pentes, marches, obstacles en maillages, lancer de rayons |
| 4. Rendu ✓ | 0.5 | Matériaux et textures du `.glb`, ombres portées, tri par matériau |

Sur ce socle, la 0.6 apporte l'édition du monde : ses objets se choisissent, se
règlent et s'enregistrent (voir « 5. Édition du monde »).

## Principes

- **Trois couches qui ne se mélangent pas.**
  - Le *moteur* (`src/moteur/`) ne connaît ni Android ni OpenGL.
  - Le *jeu* (`src/jeu.rs`) utilise le moteur.
  - La *plateforme* (`android.rs`, `renderer.rs`, `gles.rs`) affiche et reçoit les
    doigts.
  - Tout ce qui est logique se teste avec `cargo test`, sans écran.
- **Identifiants partout.**
  - Les données persistantes (personnages, tuiles, objets du monde) se désignent par
    un identifiant texte stable.
  - En mémoire, une entité est une *poignée* `Entite { indice, generation }`. Une
    poignée vers une entité détruite ne désigne plus rien, même quand son
    emplacement est réutilisé.
  - Les noms ne servent qu'à l'affichage.
- **Peu de dépendances.**
  - `gda` porte le modèle : maillages, armatures, animations, glTF.
  - Le moteur ajoute le temps réel.
  - Les seules crates externes sont `fontdue` (le texte), et `png` et `jpeg-decoder`
    (les textures, depuis la 0.5), les mêmes que dans Rustcraft.

## 1. Architecture (0.2)

### Monde

`Monde` range ses composants dans des tables typées, une par sorte de composant :
`Stock<T>`, indexé par l'indice de l'entité. C'est un « ECS » réduit au nécessaire.

- Il n'y a ni type effacé ni requête générique : chaque système est une fonction qui
  parcourt les tables dont il a besoin.
- Les composants actuels :
  - `ident` : l'identifiant stable.
  - `lieu` : position, cap, et la position au pas précédent, pour interpoler.
  - `rendu` : le maillage à dessiner.
  - `squelette` : le personnage animé et ses matrices d'os.
  - `obstacles` : des formes à ne pas traverser (boîtes, capsules, triangles).
  - `mobile` : la vitesse, la direction du pas et le rayon du corps.
  - `corps` : la hauteur du corps, sa chute, son sol (voir la physique).

### Temps

La simulation avance par **pas fixes de 1/60 s**, indépendamment de la cadence
d'affichage.

- Le temps qui reste entre deux pas sert à *interpoler* les positions dessinées.
- Ainsi, le jeu se comporte de la même façon à 30 ou 120 images par seconde.
- Au-delà de 8 pas en retard (appareil figé), le retard est abandonné plutôt que
  rattrapé.

### Ressources

`Maillages` tient les maillages à dessiner, chacun sous un identifiant.

- Un maillage a un format : *statique* (position, normale, couleur) ou *peau*
  (avec quatre os et quatre poids par sommet).
- Il a aussi une version, qui augmente à chaque changement.
- Le moteur de rendu ne renvoie au processeur graphique que ce qui a changé, et
  libère ce qui a disparu.

### Scène

À chaque image, `Scene::extraire` lit le monde et produit ce qu'il faut dessiner :

- la caméra ;
- les objets, chacun avec son maillage, sa place, et ses os s'il en a ;
- les ombres ;
- l'état des maillages.

Le moteur de rendu ne lit que cela.

## 2. Monde en tuiles (0.3)

### Coordonnées

Le monde est découpé en tuiles carrées de 64 m, repérées par `(i, j)`.

- `Monde::origine` est la tuile du joueur, et toutes les positions lui sont relatives.
  La position absolue d'un point vaut `origine × 64 + pos`, et `Monde::absolu` la
  calcule en double précision.
- Quand le joueur sort de la tuile d'origine, `rebaser` décale tous les lieux d'un
  nombre entier de tuiles, y compris leur position au pas précédent. Rien ne bouge en
  absolu, et l'interpolation ne saute pas.
- Ce qui est rattaché à une entité (relief, obstacles, maillage) est exprimé dans le
  repère de son lieu, et suit donc sans autre calcul.
- Le quadrillage du terrain a des pas de 2 et 16 m, deux diviseurs de 64 : il ne saute
  pas quand l'origine se déplace.

### Chargement

`Tuiles::suivre` reçoit la tuile du joueur.

- Elle décharge les tuiles au-delà de `garde` (3) : leurs entités sont détruites et
  leurs maillages libérés.
- Elle charge ensuite les tuiles manquantes jusqu'à `rayon` (2), les plus proches
  d'abord, avec au plus un chargement par image : lue du dépôt si elle y est, générée
  sinon.
- L'écart entre les deux rayons évite qu'une tuile oscille entre chargée et déchargée
  quand le joueur marche sur un bord.

### Contenu

Le `Contenu` d'une tuile : son relief, ses normales, ses couleurs et ses objets
(`Element` : identifiant, réglages, base dans la tuile, cap).

- Une tuile jamais modifiée vient d'une `Source`. La seule est `Campagne`. Elle somme
  des bruits calculés en coordonnées absolues, si bien que deux tuiles voisines
  s'accordent exactement sur leur bord.
- Une tuile modifiée vient du dépôt (voir « 5. Édition du monde »).
- `Tuiles` en fait les maillages, complet et simplifié.
- Chaque tuile devient des entités :
  - `tuile/i/j` porte le terrain et son `Relief`, une grille de 33 × 33 hauteurs ;
  - chaque objet est la sienne, par exemple `tuile/i/j/bloc/3`, ou `o/12` pour un
    objet ajouté dans l'éditeur.
- `Relief::hauteur` et `Relief::normale` suivent les mêmes triangles que le maillage :
  le corps marche exactement sur ce qui est dessiné.

## Animation (0.3.1)

`moteur/locomotion.rs` lit une fois, dans l'armature du personnage, ce qu'il sait
jouer, puis `animer` le fait avancer à chaque pas.

- **Allures.** La marche et la course se trouvent par leur nom. Chacune a une foulée,
  égale à sa `vitesse` (extras de l'animation) × sa durée × l'échelle. Sans `vitesse`,
  la vitesse prise est 1,4 m/s.
- **Phase commune.** La phase φ ∈ [0, 1[ avance de `dt × v / foulée(w)`. Ici, `v` est
  la vitesse du corps, et `foulée(w)` mêle les deux foulées au poids de la course `w`.
  Marche et course sont lues à la même phase, leurs contacts étant alignés.
- **Poids de la course.** Il vient de la vitesse : 0 jusqu'à 1,9 m/s, 1 à partir de
  2,4 m/s, en courbe douce entre les deux.
- **Attente.** C'est le mélange d'attente de l'armature (nommé repos, idle ou attente,
  sinon le premier), réduit à ses couches normales. Sans mélange, c'est l'animation de
  repos. Elle cède à la locomotion selon la part du mouvement.
- **Par-dessus.** Les couches additives du mélange d'attente se posent sur le tout, avec
  leur masque (`Lecteur::poser_sur` de `gda`). La respiration suit ainsi le buste pendant
  la marche, sans toucher aux jambes.
- **Glissement.** Sur `hb-01`, les pieds posés dérivent de −9 % en marche et de +3 % en
  course, quelle que soit la vitesse. Cela vient des animations elles-mêmes : le pied
  posé de « Marche » va plus vite au milieu de l'appui qu'en moyenne. Une cadence fausse
  de 20 % donnerait 14 à 31 %. Les pieds calés (0.4.1) les fixent au sol.

## Posture (0.4.2)

`moteur/posture.rs` pose deux couches calculées sur l'animation, avant les pieds.

- **Les os.** `gda::ik::buste` trouve la tête par son nom. Le cou est son parent, et le
  dos les ancêtres du cou, racine exclue.
- **L'inclinaison.** Le dos penche dans le sens de l'accélération : en avant au départ,
  en arrière au freinage, vers l'intérieur en virage. L'accélération vers l'avant vient
  de la vitesse, celle de côté de la vitesse fois la rotation du cap, toutes deux
  lissées. L'angle vaut 0,6 × atan(a/g) vers l'avant, au plus 8°, et 0,8 × atan(a/g) de
  côté, au plus 12°. Les os du dos se le partagent. En l'air, le dos se redresse.
- **Le regard.** `Squelette::regard` est un point du monde. Le jeu y met ce que vise la
  caméra, jusqu'à 30 m ; le regard est horizontal quand la caméra est à son tangage de
  départ. Le cou en prend 40 % et la tête 60 %, chacun dans ses limites :
  - jusqu'à 70° de côté à l'arrêt, la moitié à 3 m/s ;
  - 35° vers le haut, 40° vers le bas ;
  - plus de 110° en arrière, le personnage regarde devant lui.
- Les angles sont lissés, et le moniteur montre l'inclinaison (« Buste ») et le regard.

## Pieds calés (0.4.1)

`moteur/pieds.rs` est la dernière couche de la pose, après la locomotion et la posture. `animer` la
joue au monde tel qu'il est dessiné, à la place interpolée du corps.

- **Les jambes.** `gda::ik::jambes` les trouve une fois par personnage : chaque os dont le
  nom dit un pied, avec son parent (le genou) et son grand-parent (la cuisse).
- **Le sol sous chaque pied.** La cheville animée est portée sur ce qu'il y a sous elle :
  relief, dalle, rampe. En l'air, ou au-dessus d'un vide plus bas que 45 cm, le pied joue
  l'animation.
- **Le pied tenu.** Une cheville à moins de 3 cm de sa hauteur debout est posée : le pied
  reste là où il s'est posé, au monde, jusqu'à ce que l'animation le lève. Il rejoint
  alors l'animation en 0,15 s. S'il s'éloigne de plus de 25 cm de l'animation (le corps
  tourne sur place), il se libère. Les pieds tenus suivent les changements d'origine.
- **Le bassin.** La racine descend pour que le pied le plus bas atteigne son sol, de
  45 cm au plus ; seuls les pieds près du sol comptent. La hauteur vue du corps suit sa
  hauteur vraie en douceur : une marche montée d'un coup ne saute pas.
- **Les jambes plient** par `gda::ik::deux_os` : le genou autour de son axe X, dans ses
  limites, puis la cuisse vers la cible.
- **Le pied** garde la rotation que lui donnait l'animation, inclinée selon la pente
  tant qu'il est à moins de 10 cm du sol, dans les limites de la cheville.
- Le moniteur montre chaque pied (tenu, lâché, levé) et la descente du bassin.

## 3. Physique (0.4)

`moteur/physique.rs`. Le corps n'est pas un objet libre : il va où on le commande, et
la physique corrige.

### Corps

- Un **corps** est une capsule verticale, posée par ses pieds sur `Lieu::pos` : le
  rayon vient de `Mobile::rayon`, la hauteur de `Corps::hauteur`. Celle du joueur suit
  celle de son modèle.
- `Corps` garde aussi la vitesse de chute, s'il est au sol, la normale de ce sol et la
  vitesse de sa glissade.
- `physique::poser` pose chaque corps sur ce qu'il y a de plus haut à sa verticale :
  au départ, ou quand le monde a changé sous lui.

### Obstacles

`Monde::obstacles` tient, par entité, des `Forme`, dans le repère de son lieu (sans
rotation) :

- `Boite` : une boîte alignée sur les axes ;
- `Capsule` : verticale, posée en `base` ;
- `Triangles` : un maillage de collision. Une grille au sol de cases de 2 m dit quels
  triangles passent dans chacune ; seuls ceux des cases voisines du corps sont lus.

Le relief des tuiles est le sol, toujours.

### Un pas

1. **Pente.** Si le corps est sur une surface plus raide que `PENTE_MAX` (45°), la
   part du pas qui monte est retirée.
2. **Avance.** Le corps avance à l'horizontale. Il est repoussé hors des obstacles
   qui dépassent ses pieds de plus d'une marche (`MARCHE`, 35 cm) et glisse le long.
   Pour les triangles, seuls les raides comptent comme murs : les autres sont du sol.
   Le contact se teste par des sphères empilées le long du corps.
3. **Sol.** Le sol est la surface la plus haute sous le corps, une marche au-dessus
   des pieds au plus. Si c'est une marche raide, le corps n'y monte pas et reste sur
   place.
4. **Pose ou chute.** Au sol, le corps se pose sur le sol trouvé, même un peu plus bas
   (une marche plus 10 cm par pas) : il suit les descentes. Plus bas, il tombe à
   9,81 m/s² jusqu'à rencontrer une surface.
5. **Glissade.** Sur une pente trop raide, la pesanteur fait glisser le corps le long
   de la pente. Ailleurs, la glissade s'éteint en une fraction de seconde.

`physique::deplacer` fait ce pas pour tous les corps, après `marcher`, qui ne règle
plus que la vitesse et le cap.

### Rayons

`physique::lancer(monde, origine, direction, portée)` rend le premier `Impact` :
distance, point, normale et entité touchée (`None` pour le relief). Il touche les
boîtes, les capsules, les triangles de la grille et le relief.

Le jeu s'en sert de deux façons :

- la caméra est ramenée devant le premier obstacle entre le joueur et elle ;
- le moniteur montre ce que vise le centre de l'écran, jusqu'à 80 m.

### Ce que le moniteur montre

- **Appui** : ce qui porte le joueur, le relief ou l'identifiant d'un objet.
- **Pente** : la pente sous ses pieds.
- **Visé** : ce que vise le centre de l'écran, et sa distance.

## 4. Rendu (0.5)

`renderer.rs` ne lit que la `Scene`. À chaque image, il dessine la carte d'ombre, puis
les parties triées, puis l'interface.

### Matériaux et textures

- Un maillage peut être découpé en `Partie`s : une suite de sommets d'un même `Materiau`,
  c'est-à-dire une texture, une opacité et un mode d'alpha (`Opaque`, `Masque`,
  `Melange`).
- `Textures` tient les images décodées, sous leur identifiant et versionnées comme les
  maillages. Le moteur de rendu les charge avec leurs mipmaps, en filtrage trilinéaire,
  et répétées.
- Le personnage (`personnage.rs`) range ses triangles par matériau du `.glb`, les
  opaques d'abord, et garde leurs coordonnées de texture. Une texture est décodée par
  `image.rs` (PNG ou JPEG, réduite à 1024 px de côté au plus). Les textures du joueur
  sont enregistrées sous `personnage/<id>/<rang du matériau>`.
- La couleur d'un sommet est celle de son matériau ; la texture la multiplie. Un
  matériau d'opacité inférieure à 1 est mêlé, une texture qui a de la transparence est
  coupée net à mi-opacité. Ces deux sortes se voient des deux côtés.
- Le mannequin intégré a une texture sur sa veste, un sergé : l'exemple toujours
  présent.

### Tri

Chaque partie de chaque objet devient un appel.

- Les opaques et les masqués sont triés par programme, puis par texture : chaque
  programme et chaque texture ne se choisissent qu'une fois par image.
- Les mêlés viennent ensuite, du plus loin au plus proche, sans écrire la profondeur.

### Ombres portées

- Le soleil (`Scene::soleil`) a une direction et un centre : le jeu y met le joueur.
- Sa carte de profondeur fait 2048 × 2048 texels et couvre un carré de 56 m autour du
  centre, sur 100 m de profondeur. Elle avance par texels entiers : les bords des ombres
  ne scintillent pas quand le joueur bouge (`Soleil::matrice`).
- Le décor et les personnages proches y sont dessinés, avec un décalage de profondeur.
  Le terrain reçoit les ombres sans en porter.
- Chaque surface lit son ombre en 3 × 3 comparaisons filtrées, après un petit décalage
  le long de sa normale. Vers le bord du carré, l'ombre s'efface.
- Si le processeur graphique refuse la carte, tout est dessiné sans ombres.
- Les ombres rondes de la 0.3 au pied des personnages ont disparu.

### Tuiles lointaines

- Chaque tuile a deux terrains : le complet (32 × 32 carrés) et le simplifié, un point
  sur 4 (8 × 8 carrés), entouré d'une jupe qui descend de 1,5 m et cache les fentes avec
  le terrain complet voisin.
- Les tuiles à plus d'une tuile du joueur (`PROCHE`) dessinent le simplifié. Le
  changement se fait dans `Tuiles::suivre`, en même temps que le chargement.
- Les objets restent entiers partout.
- Au départ, 29 000 triangles sont dessinés au lieu de 76 000.

## 5. Édition du monde (0.6)

La seconde pastille, au crayon, ouvre le panneau de gauche sur l'onglet Éditeur. On y
choisit un objet, on le règle, et le monde s'enregistre seul.

### Objets

- Un objet est une entité. Son composant `Objet` (`src/moteur/objets.rs`) contient :
  - un modèle du catalogue (`Genre`) : bloc, dalle, poteau, rampe ;
  - ses dimensions et sa couleur.
- Son repère : la base au sol en (0, 0, 0), l'avant vers +z, comme les personnages.
- Son maillage est partagé par les objets de mêmes réglages, sous un identifiant tiré
  des réglages (`objet/bloc/1600x1600x1600/d1c7b3`). `balayer` libère ceux que plus rien
  ne dessine.
- Ses obstacles suivent son cap :
  - une boîte tournée d'un quart de tour reste une boîte (la physique ne connaît que
    des boîtes alignées) ;
  - de travers, elle devient des triangles, comme la rampe ;
  - un poteau reste une capsule.
- `Tuiles::modifier`, `ajouter` et `oter` changent le monde et marquent les tuiles
  touchées. Un objet appartient à la tuile qui contient sa base : déplacé au-delà d'un
  bord, il change de tuile, et les deux tuiles sont à réécrire.

### Dépôt

`Depot` (`src/moteur/depot.rs`) range le monde dans un dossier. Sur Android, c'est le
dossier privé de l'application, sous `monde/`. Hors Android, le monde reste en mémoire.

- `monde.json` est la fiche du monde : son identifiant, son nom, la graine du
  générateur, le prochain numéro d'objet (`suivant`), et où était le joueur.
- `tuiles/<i>_<j>.json` contient une tuile modifiée tout entière : ses hauteurs, ses
  couleurs (en hexadécimal) et ses objets, chacun avec son identifiant, son modèle,
  sa base, son cap, ses dimensions et sa couleur.
  - Les normales se recalculent à la lecture.
  - La tuile ne doit plus rien au générateur.
- Chaque fichier est écrit à côté puis renommé : une écriture coupée ne laisse rien
  à moitié.
- Une tuile modifiée s'écrit dans trois cas :
  - 2 s après le dernier changement (`DELAI`) ;
  - quand elle se décharge ;
  - quand l'application passe en arrière-plan. La fenêtre se détruit, et
    `App::enregistrer` écrit aussi la place du joueur.
- Au lancement suivant, le joueur reprend où il était. Les tuiles écrites sont relues
  à la place des générées.
- Un fichier illisible est remplacé par la tuile générée, et l'erreur s'affiche dans
  l'éditeur.

### Éditeur

`src/editeur.rs`.

- Un appui bref dans la vue lance un rayon de l'œil par le point touché
  (`Camera::rayon`, puis `physique::lancer`).
  - L'interface signale un appui bref sur l'espace de travail, ou sur la manette,
    s'il ne glisse pas et qu'aucun second doigt n'est venu (`Ui::appui_bref`).
  - Tourner la vue et zoomer ne choisissent donc rien.
- Ce que fait l'appui dépend du mode :
  - *choisir* : l'objet touché est choisi, ou plus rien si l'on touche le sol ;
  - *déplacer* : l'objet choisi se pose là, le rayon ne le voyant pas lui-même ;
  - *ajouter* : un objet neuf du modèle se pose là, de face, au quart de tour. Il
    reçoit un identifiant neuf (`o/<suivant>`).
- L'objet choisi se dessine teinté d'orangé (`Dessin::choisi`, `u_choisi`).
- Le panneau montre l'objet choisi :
  - son identifiant, sa tuile, sa place, son altitude ;
  - les liens Déplacer, Dupliquer, Supprimer, Laisser ;
  - ses réglages : le cap (au pas de 5°), chaque dimension (au pas de 5 cm), l'altitude
    (au pas de 10 cm), la couleur ;
  - en dessous, le catalogue (AJOUTER), puis l'état de l'enregistrement et « Annuler ».
- « Annuler » défait le dernier changement (50 au plus). Les retours d'un même curseur
  se fondent en un.
- Fermer l'éditeur laisse l'objet choisi.
- Le relief se retouche dans l'onglet Topologie (0.9).

## 6. Manettes de jeu (0.7)

### Plateforme

`android.rs` trie chaque événement selon sa source (`AInputEvent_getSource`).

- Un événement de manette ou de joystick n'est jamais un doigt. Ses touches restent au
  jeu : B ne fait pas Retour.
- La première fois qu'une manette se manifeste, Java la décrit
  (`InputDevice.getDevice`). On obtient :
  - son nom ;
  - son **descripteur**, un identifiant stable d'une connexion à l'autre ;
  - les axes qu'elle a (`getMotionRange`).
- Les touches arrivent sans leurs répétitions. Les axes arrivent ensemble, avec les 12
  lus (`manette::AXES`).
- Toutes les 2 s, la liste est relue : une manette qu'Android ne connaît plus est
  partie.

### Actions et profils

`src/manette.rs` ne connaît pas Android.

- Le jeu lit des *actions*, chacune sous un identifiant (`ACTIONS`) :
  - deux sticks : `marcher`, `vue` ;
  - des boutons : `approcher`, `eloigner`, `choisir`, `annuler`, `moniteur`, `editeur`,
    `reglages`, `fermer`.
- Une `Entree` est ce qu'une action écoute :
  - une touche ;
  - un demi-axe : une gâchette, ou un côté de la croix ;
  - un stick, c'est-à-dire deux axes.
- L'assignation automatique suit la disposition normalisée d'Android, d'après les axes
  présents :
  - le stick droit sur Z/RZ, sinon RX/RY ;
  - les gâchettes sur LTRIGGER/RTRIGGER, sinon BRAKE/GAS, sinon L2/R2.
- La capture assigne la prochaine touche, ou le prochain axe poussé au-delà de 0,6 de
  son repos (une gâchette au repos à −1 est reconnue). Pour un stick, l'axe désigne sa
  paire. Une action qui perd son entrée prend l'ancienne de l'autre.
- Le profil (liens, zone morte radiale, vitesse et sens de la vue) s'écrit dans
  `reglages/manettes/<descripteur>.json` à chaque changement. Une action absente du
  fichier garde l'assignation automatique.
- La manette qui a servi en dernier commande.
- `commandes()` donne à chaque image les sticks, les boutons tenus et ceux tout juste
  enfoncés. La touche qui termine une capture ne déclenche rien.

### Panneau

- Chaque pastille ouvre le panneau sur ses propres onglets :
  - le pouls sur Moniteur ;
  - le crayon sur Éditeur ;
  - l'engrenage sur Manette (d'autres réglages pourront s'y ajouter).
- Le dossier de l'application contient `monde/` et `reglages/`.

### Affichage (0.8.1)

L'onglet Affichage des réglages (`reglages::Affichage`, écrit dans
`reglages/affichage.json`) règle deux choses.

- **La distance** est le rayon des tuiles chargées, de 1 à 5 tuiles. La garde suit
  une tuile plus loin.
  - `App::frame` la donne à `Tuiles` à chaque image ; `suivre` charge une tuile par
    image.
  - Le plan lointain de la caméra recule avec elle.
- **Le brouillard** passe au shader par `Scene.brouillard` (`u_brouillard`) :
  `f = 1 − exp(−(2d/B)²)`, soit 98 % de ciel à la distance B.
  - B est borné de 30 m à (distance + ¼ de tuile), pour que le bord du monde reste
    caché.
  - Quand la distance change, B garde sa part de la distance.

## 7. Décors et bâtiments en .glb (0.8)

### Catalogue

`src/catalogue.rs` décode un `.glb` sans squelette en `Decor` et le garde dans
`catalogue/` du dossier de l'application.

- `catalogue/<id>.glb` contient le fichier tel qu'il est arrivé.
- `catalogue/catalogue.json` donne, pour chaque entrée :
  - l'identifiant ;
  - le nom affiché ;
  - le fichier ;
  - l'empreinte, en FNV-1a sur 64 bits ;
  - le nombre de triangles.
- L'identifiant vient de `extras.sage.id`, sinon il est donné en `glb/<n>`. Importer à
  nouveau le même fichier ne fait rien, puisque l'empreinte est la même.
- `jeu::charger` aiguille le fichier :
  - un `.glb` avec squelette est un personnage ;
  - un `.glb` sans squelette entre au catalogue, puis l'éditeur s'ouvre sur
    `Ajouter(Modele(k))`.
- À l'ouverture, chaque entrée est décodée. Une entrée illisible laisse son modèle
  absent.

### Pose

- La géométrie passe au repère du modèle : base en (0, 0, 0) et avant vers +z, comme
  en glTF.
- Le point le plus bas va à y = 0. Le modèle est recentré en x/z si son origine tombe
  hors de son emprise.
- Les normales sont lissées, sauf sur une arête vive de plus de 40°.
- Les couleurs des matériaux passent par `srvb()`.

### Moteur

`moteur/modeles.rs` tient le registre `Modeles`.

- `ModeleId` est une poignée sur `Modele { id, nom, gabarit, version }`. Le `Gabarit`
  contient :
  - les sommets au `Format::Texture` (position, normale, couleur, uv) ;
  - les parties par matériau ;
  - les triangles de collision ;
  - la boîte.
- Un objet `Genre::Modele(k)` garde son échelle uniforme dans `dim = [s; 3]`, bornée de
  0,01 à 100.
- Le maillage `modele/<id>/<version>` est partagé par toutes les instances. L'échelle
  passe par `Rendu.local`. Une nouvelle version (« Remplacer le fichier du modèle »)
  refait les objets par `tuiles::rafraichir`.
- La collision est un obstacle `Triangles` : ceux du gabarit, mis à l'échelle et
  tournés au cap.
- Un modèle inscrit sans gabarit se dessine avec le maillage `modele-absent`, une boîte
  grise d'un mètre, et heurte comme elle.
- Le rendu dessine `Format::Texture` par le programme `matiere`. Ce programme gère :
  - les textures ;
  - l'opacité ;
  - le masque ;
  - la teinte de sélection ;
  - les ombres.

### Mémoire

- La tuile écrit l'élément d'un modèle en
  `{"id","genre":"modele","modele":<id>,"pos","cap","echelle"}`, sans sa géométrie.
- À la lecture, un identifiant inconnu est inscrit sans gabarit : l'objet attend son
  modèle.

### Faces (0.8.2)

- `gda::Maillage::a_l_envers` mesure le volume signé des faces depuis le centre de leur
  boîte. Le modèle est à l'envers si ce volume est inférieur à −¼ de la somme des
  volumes sans signe ; un sol ou un panneau seul, sans volume, ne l'est jamais.
- L'import retourne alors les faces (`Decor.retourne`). `Entree.retourne` garde le
  choix dans `catalogue.json` (`"faces": "endroit"` ou `"retournees"`) ; absent, il est
  décidé à la lecture.
- Les normales étant recalculées depuis l'ordre des sommets, retourner les faces suffit.
- `Materiau.deux_cotes` vient de `doubleSided`. Le programme `matiere` ne supprime pas
  l'envers de ces parties et y retourne la normale.
- gda remet aussi à l'endroit les faces d'un nœud à matrice miroir, comme le veut glTF.

### Limites

- Sont refusés :
  - les maillages compressés (Draco, meshopt) ;
  - les textures KTX2 et WebP.
- Les animations sont ignorées.
- Au-delà de `TRIANGLES_LOURD` (50 000), le modèle est signalé comme lourd.

## 8. L'éditeur en pause, la topologie et les poignées (0.9)

### Pause

- `App::edite` (le panneau ouvert sur l'éditeur) met le jeu en pause : l'horloge ne
  compte plus, ni la marche ni l'animation n'avancent.
- La caméra tourne autour de `App::visee`, un point du sol pris sous le joueur à
  l'ouverture, un mètre au-dessus du relief, jusqu'à 150 m et presque à la verticale.
  La manette et deux doigts qui glissent (`Gesture::pan`) le déplacent ; les tuiles le
  suivent.
- Le joueur n'est pas dessiné, les ombres se centrent sur le point visé.
- On voit jusqu'à `EDITION_TUILES` (16 tuiles, 1 km), sans brouillard ; 4 tuiles se
  chargent par image. Les tuiles lointaines n'ont que leur maillage simplifié : le
  complet ne se fait qu'à `PROCHE` tuiles du centre, et s'en va au-delà de la suivante.
- Refermé, la caméra revient au joueur ; si le sol est monté sous lui, il est reposé
  dessus.

### Topologie

`src/terrain.rs`, onglet Topologie de l'éditeur.

- Un pinceau rond (`Pinceau` : outil, rayon de 2 à 24 m, force) agit sur les points de
  la grille sous son disque, avec le poids `(1 − (d/r)²)²`.
  - Monter, Creuser : 2 m/s au centre, à pleine force.
  - Aplanir : vers la hauteur du premier point du coup (`niveau`).
  - Adoucir : vers la moyenne des quatre voisins, pris dans la tuile d'à côté s'il le
    faut.
  - Peindre : vers la couleur choisie (`TEINTES`).
- Tout se calcule sur l'état d'avant le coup : un point du bord, présent dans deux
  tuiles, reçoit le même calcul dans chacune, et les tuiles restent jointes.
- `Tuiles::retoucher` refait les maillages complet et simplifié de la tuile, remet son
  relief (la physique le lit tel quel) et la marque à réécrire.
- Un outil pris, le doigt seul dans la vue (`Ui::doigt_seul`) peint au lieu de tourner ;
  deux doigts zooment et déplacent toujours la vue. Le cercle du pinceau se dessine sur
  le relief.
- Un coup, du doigt posé au doigt levé, est une étape d'annulation : `Retour::Relief`
  garde les tuiles touchées telles qu'avant lui.
- En Topologie, la caméra va jusqu'à la verticale et s'éloigne jusqu'à 2 km
  (`TOPO_LOIN`). Son haut se penche avec elle (`[-sin λ sin φ, cos φ, -cos λ sin φ]`),
  ce qui la garde juste à la verticale. Seul le relief est dessiné (les maillages
  `tuile/…`).
- Les courbes de niveau se tracent dans le programme du terrain (`u_isolignes`) : un
  trait d'un pixel tous les `u_isolignes` mètres d'altitude, un plus fort tous les cinq,
  à la place du quadrillage. L'écart suit la distance de la caméra (`isolignes` : environ
  d/40, au pas de 1, 2, 5, 10, 25, 50, 100 m). Elles suivent le pinceau sans calcul à
  part.
- L'altitude sous le doigt s'affiche à côté de lui pendant le coup.
- Les objets ne suivent pas le sol retouché : leur enfoncement se règle dans l'onglet
  Ajout.

### Poignées

- L'objet choisi, en mode *choisir*, porte cinq poignées dessinées par-dessus la vue
  (`Camera::ecran` projette un point du monde) :
  - le disque blanc glisse sur le plan horizontal de sa base ;
  - les flèches rouge et bleue le poussent le long de ses axes x et z (tournés de son
    cap), la verte en hauteur, au pas de 5 cm : le déplacement du doigt est projeté sur
    la flèche vue à l'écran ;
  - la pastille jaune de l'anneau le tourne autour de sa base, au pas de 5°.
- Les bras gardent à peu près la même taille à l'écran (16 % de la distance à l'œil).
- Chaque poignée est une zone tenue au doigt (`Ui::drag`) : la toucher ne choisit rien
  d'autre. Un geste, du doigt posé au doigt levé, est une étape d'annulation.
