# S.A.G.E.

Moteur de jeu en Rust pour Android : une application native, en OpenGL ES 3, qui
fait vivre les personnages créés dans [Rustcraft](https://github.com/s1pierro/rustcraft).
Il vise un monde ouvert, avec d'autres valeurs que la violence.

![Le mannequin marche dans le décor de S.A.G.E.](docs/apercu.png)

**Télécharger l'APK** : [sage-0.51.apk](https://github.com/s1pierro/sage/releases/download/v0.51/sage-0.51.apk)
(Android 8 ou plus récent, arm64).

## Version 0.51 : la voirie seule

- Dans l'onglet Voirie, **Tout / Voirie seule** : en voirie seule, la vue ne garde que
  le terrain des tuiles et leurs rues (chaussées, caniveaux, bordures, trottoirs,
  accotements) ; bâtiments, arbres, objets, ponts et lointain s'effacent. Les murs
  arrêtent toujours le joueur.

## Version 0.50 : le trottoir jusque dans l'angle

- Le trottoir va jusqu'à la façade en regardant droit devant depuis chaque point, en
  s'éloignant de la chaussée : dans l'angle d'un carrefour, ce regard part en diagonale
  et rejoint l'angle du bâtiment ; plus de triangle de terrain entre le bâtiment et la
  bordure arrondie.
- Entre deux façades proches (jusqu'à 10 m de part et d'autre), la limite du trottoir se
  tire de l'une à l'autre, comme avant la 0.49 ; pas derrière un bâtiment.

## Version 0.49 : les rues d'un seul tenant

- Chaussées, caniveaux, bordures, trottoirs et accotements sont désormais tirés d'un
  **même champ**, calculé pour toutes les rues à la fois : la chaussée et le trottoir
  partagent exactement la même bordure, et aux carrefours les trottoirs des rues qui se
  croisent ne font qu'une surface, **sans trou ni recouvrement**.
- Les angles des carrefours s'arrondissent : **Angles des carrefours** dans l'onglet
  Procédures, section Chaussées (4 m d'origine, de 0 à 10 m ; 0 : angles vifs). Deux
  morceaux d'une même rue, ou deux rues parallèles, ne s'arrondissent pas entre eux.
- Le trottoir va jusqu'à la façade d'un bâtiment à moins de la portée (l'enveloppe du
  bâtiment et de la bordure en face), sinon il garde sa largeur.
- Au raccord de deux morceaux d'une rue coudée, la chaussée ne s'ouvre plus.
- L'accotement ne se pose plus sous la chaussée et les trottoirs, seulement là où il se
  voit.

## Version 0.48 : la bordure pavée sur son épaisseur

- La face de la bordure, côté chaussée, porte les mêmes pavés ternes que le rang du
  trottoir, qui s'y replient sans raccord à l'arête.

## Version 0.47 : bordures pavées à leurs mesures

- Le long de la bordure, le trottoir porte un rang de pavés ternes de **20 cm de large
  et 1 m de long**, puis le bitume jusqu'à la façade.
- Côté chaussée, le caniveau pavé fait **15 cm de large**, en pavés de **90 cm de
  long**.
- La texture des pavés est un rang de quatre pavés ; chaque bande choisit la longueur
  de ses pavés.

## Version 0.45 : bordures pavées

- Le long de la bordure, le trottoir porte un **rang de pavés clairs de 20 cm de large
  et 50 cm de long**, puis le bitume sombre jusqu'à la façade.
- Côté chaussée, au pied de la bordure, un **caniveau pavé** de 15 cm (pavés de 50 cm,
  un peu plus sombres).
- Les deux utilisent la même texture générique : un rang de pavés, étiré à la largeur
  de la bande.

## Version 0.44 : le profil des routes lissé sur tout le réseau

- La hauteur des routes se lisse sur tout le réseau à la fois, sans s'arrêter au bout
  de chaque voie : un carrefour est un seul point, d'une seule hauteur, lissé avec
  toutes les routes qui y passent. Les bouts des ponts gardent la hauteur du tablier.
  Les rues se lissent entre elles d'abord, puis les voies piétonnes, chemins et
  sentiers, accrochés aux rues (ils ne les tirent plus vers le haut ou le bas).
- Onglet Procédures, Terrassement : **Profil des routes lissé** (20 m d'origine, de 0 à
  60 m). À Rouen, avec 20 m, les cassures de pente (plus de 8 % de différence d'un
  segment à l'autre) passent de 1 563 à 236, et les pentes de plus de 25 % de 205 à 12.
  (Il faut régénérer les régions.)

## Version 0.43 : des trottoirs revêtus

- Les trottoirs sont revêtus de deux petites textures génériques, faites au lancement et
  réutilisables : une **bande de pavés clairs** (50 cm, pavés de 25 × 12,5 cm en rangs
  décalés) le long de la bordure, puis du **bitume sombre** jusqu'à la façade, comme
  dans le Rouen des années 1990. Le chant côté façade est sombre lui aussi.

## Version 0.42 : les voies piétonnes cèdent aux rues

- Les voies piétonnes, chemins et sentiers n'arrêtent plus un trottoir : celles qui
  longent une rue (les trottoirs notés à part dans OSM) passent sous lui, et il va
  jusqu'à la façade.
- Au terrassement, les plateformes des rues et les abords des bâtiments passent avant
  celles des voies piétonnes : une voie piétonne plus basse ne creuse plus le bord de la
  rue. Entre une voie piétonne et une rue, pas de travée. (Il faut régénérer les régions
  pour le terrassement.)

## Version 0.41 : les tronçons de voirie

- Dans l'onglet Voirie, toucher un fil choisit un **tronçon** : la part de la route d'un
  carrefour à l'autre. Il se dessine le long du fil : son axe épais, les bords de sa
  chaussée (à sa largeur), ceux de ses trottoirs s'ils sont réglés, et ses deux bouts.
- Le panneau montre sa longueur, son genre et sa largeur d'origine, et règle pour ce
  tronçon seulement :
  - sa **priorité** (grande route, route, service, piétonne, chemin, sentier) ;
  - la **largeur de la chaussée** (1,5 à 20 m) ;
  - ses **trottoirs** : selon le quartier, sans, ou d'**au moins** une largeur (0,5 à
    6 m, même hors des quartiers).
- Chaussée et trottoirs se refont aussitôt ; la largeur et la priorité changent aussi
  le terrassement, que le panneau propose de régénérer. « Valeurs d'origine » efface la
  retouche. Les retouches sont gardées dans `monde/voirie.json`, à part des régions.

## Version 0.40 : l'onglet Voirie

- L'éditeur a un cinquième onglet, **Voirie**, pour éditer les routes. Pour commencer :
  quand il est ouvert, un **fil** suit l'axe de chaque route à moins de 192 m du point
  visé, par-dessus la vue, de la couleur de son genre (orange les grandes routes, jaune
  les routes, blanc les voies de service, bleu les voies piétonnes, brun les chemins,
  vert les sentiers).
- L'onglet compte les routes alentour par genre, et leur longueur.
- Les onglets se serrent quand la place manque.

## Version 0.39 : les abords des bâtiments

- Avec le terrassement en talus, un bâtiment dont un coin est à moins d'une distance
  réglable du bord d'une rue (Procédures › « Bâtiments au niveau de la rue », 12 m
  d'origine, 0 pour aucun) a son **abord** terrassé à la hauteur de la rue : sous lui,
  et entre lui et la rue ; un talus le rejoint au-delà. Le trottoir va à plat jusqu'à
  la façade.
- Un bâtiment dont le sol a été remonté jusqu'à la rue monte d'autant : il garde sa
  hauteur au-dessus du sol. Là où le sol descend, son pied descend.
- Comme tout le terrassement, cela entre dans les régions en les régénérant (bâtiments
  venus du lieu ; les retouchés, enrichis ou posés restent tels quels).

## Version 0.38 : le terrassement au choix, et régénérer sur place

- Procédures › Terrassement : **Talus** (plateforme et talus de la 0.35, avec les
  travées), **Fondu** (le sol aplani sous la chaussée et fondu sur les bas-côtés, comme
  avant) ou **Aucun** (le relief tel quel).
- Quand le terrassement a changé, l'onglet propose de **régénérer** la région visée,
  avec ses voisines, ou toute la carte, et suit l'avancement. Le besoin est gardé
  jusqu'à la régénération, même après avoir fermé l'application.
- La chaussée et l'accotement portent ce qui marche ou roule (le sol dessous ne suit
  pas toujours : terrassement fondu ou absent, région pas encore régénérée).

## Version 0.37 : l'onglet Procédures

- L'éditeur a un quatrième onglet, **Procédures**, pour régler et essayer ce qui se
  bâtit autour des routes :
  - **Chaussées** maillées ou seulement peintes ; avec ou sans **accotements**, et leur
    largeur ;
  - **Trottoirs** ou non ; leur largeur sans façade, jusqu'où ils vont chercher la
    façade, la hauteur de la bordure ;
  - **Terrassement** : la plateforme, la pente et la portée des talus.
- Chaussées, accotements et trottoirs se refont aussitôt sur les tuiles chargées ; le
  terrassement entre dans le relief des régions, il s'applique en les régénérant.
- « Valeurs d'origine » remet tout comme avant. Les réglages sont gardés
  (`reglages/procedures.json`).

## Version 0.36 : les chaussées

- Les grandes routes, les rues et les voies de service ont leur propre **chaussée**,
  posée sur la plateforme du terrassement, aux bords nets (au lieu d'être peintes sur la
  grille du sol). Les carrefours se fondent d'eux-mêmes ; une route s'arrête net à son
  bout.
- De chaque côté, un **accotement** de 2 m (terre et gravier à la campagne, gris en
  ville) rejoint le terrain ; il finit par un chant qui descend sous le sol, sans jour
  sur les remblais. En ville, les trottoirs le recouvrent là où ils sont.
- Calculées à la volée, tuile par tuile : rien à régénérer de plus que pour la 0.35.
  Chemins, sentiers et voies piétonnes restent peints sur le sol.

## Version 0.35 : le terrassement des routes

- Le sol se terrasse sous les routes du lieu réel : la chaussée et, de chaque côté, la
  place d'un trottoir ou d'un accotement (2 m, 50 cm pour les chemins) sont à plat ;
  au-delà, un talus d'au plus 2 pour 3 rejoint le relief, en déblai ou en remblai, sur
  20 m au plus. Dans l'eau, pas de talus : la berge reste un quai.
- Le profil des routes est lissé une fois de plus, et toutes les routes d'un carrefour
  s'y retrouvent à la même hauteur (celle du tablier au bout d'un pont).
- Deux routes qui se croisent sans carrefour à plus d'un mètre d'écart : le sol est à
  la hauteur de la plus basse, la plus haute passe sur une **travée** (un tablier avec
  ses garde-corps) au-dessus de la plateforme de l'autre.
- Un bâtiment au bord d'un talus descend son pied jusqu'au sol terrassé.
- Le terrassement se calcule en relisant le lieu (`lieu.reel` inchangé) : il faut
  **régénérer** les régions (Carte › Régénérer) pour qu'il apparaisse ; les retouches
  sont gardées.

## Version 0.34 : l'heure du jour

- **Réglages → Affichage → Soleil** : un curseur règle l'heure, au quart d'heure, de
  0 h à 24 h. Le soleil suit sa course d'un jour d'été à la latitude du lieu (celle de
  Rouen pour l'île) : levé au nord-est, au sud à midi, couché au nord-ouest. Les ombres
  portées tournent et s'allongent avec lui.
- Au lever et au coucher, la lumière se dore et le ciel rosit ; la nuit, plus de
  lumière directe ni d'ombres, un ciel bleu sombre où se fond le lointain.
- L'heure est gardée avec les autres réglages d'affichage.

## Version 0.33 : les trottoirs

- Les grandes routes, les rues et les voies de service des quartiers (zones habitées
  et d'activités) ont des **trottoirs** : une dalle à 15 cm au-dessus de la chaussée,
  avec sa bordure, qui va jusqu'à la façade du bâtiment voisin quand elle est à moins
  de 12 m, sinon large de 2 m. Entre deux façades proches, la largeur se tire de l'une à
  l'autre ; elle est lissée le long de la rue. À la campagne, pas de trottoir.
- Un trottoir s'arrête à la chaussée d'une autre rue (aux carrefours) et à l'eau. Il se
  monte sans sauter, et arrête ce qui roule ou marche comme le reste du décor.
- Calculés à la volée tuile par tuile, comme les arbres écartés : un bâtiment posé,
  déplacé ou agrandi refait aussitôt le trottoir devant lui. Rien ne s'écrit, les
  régions n'ont pas à être refaites.
- La chaussée est peinte pleine jusqu'à son bord.

## Version 0.32 : l'emprise tirée du modèle

- Un bâtiment enrichi d'un `.glb` prend la forme de son modèle : son contour est
  l'emprise du modèle vu du dessus (son enveloppe), sa hauteur celle du modèle. Le
  choisir au toucher, écarter les arbres, son bloc lointain et la place qu'il garde à
  la régénération suivent le modèle, et non plus l'ancien contour.
- Sa forme procédurale d'avant est gardée dans sa définition : « Revenir au
  procédural » la rend. Les bâtiments enrichis avant la 0.32 prennent l'emprise de
  leur modèle à la lecture.

## Version 0.31 : les arbres s'écartent des bâtiments

- Un bâtiment posé, déplacé, tourné ou agrandi écarte aussitôt les arbres qu'il
  recouvre (à un mètre de ses murs près) : ils ne se voient plus et n'arrêtent plus le
  joueur. Démoli ou remis en place, il les rend.
- Rien ne s'écrit pour cela : les arbres restent dans leur région, et c'est l'emprise
  des bâtiments debout qui décide, tuile par tuile, de ceux qui poussent.

## Version 0.30 : de nouveaux bâtiments

- Dans un monde matérialisé, l'onglet Ajout de l'éditeur pose un **bâtiment neuf** là
  où l'on touche : une maison de 10 × 8 m à deux pans, face à la caméra, le pied
  calé sous le sol le plus bas de son contour.
- Il se règle comme les autres (toit, hauteurs, cap, couleurs, déplacement), s'enrichit
  d'un `.glb`, et chaque bâtiment non enrichi gagne deux réglages, **Largeur** et
  **Profondeur**, qui étirent son contour.
- Il s'écrit dans sa région en instance `provenance: "main"`, `retouche: true` : une
  régénération le garde, et n'y refait pas de bâtiment. Démoli (ou son ajout annulé),
  il quitte le fichier, sa définition avec lui.

## Version 0.29 : régénérer un fragment du monde

- Dans l'onglet Carte de l'éditeur, un monde matérialisé se **régénère par morceaux** :
  la région du point visé (1 km de côté), ou avec ses 8 voisines. Elle est refaite à
  partir du lieu réel, dans un autre fil, puis les tuiles et le lointain se refont.
- Les **bâtiments retouchés, enrichis ou démolis restent** tels quels, avec leurs
  modèles et leurs textures ; un bâtiment refait qui tomberait à leur place ne revient
  pas, ni un arbre dans un bâtiment gardé.

## Version 0.28 : enrichir les bâtiments

- Le panneau d'un bâtiment l'**exporte en `.glb`** (dans Téléchargements, Android 10 ou
  plus) : posé à l'origine, sans rotation, pour le retravailler dans Rustcraft ou
  Blender. Un bâtiment déjà enrichi s'exporte avec son modèle et ses textures.
- **Remplacer par un .glb…** : le modèle choisi devient le niveau détaillé du bâtiment,
  avec ses matériaux et ses textures. Il se voit et arrête le joueur tout de suite, se
  choisit, se déplace, tourne et se démolit comme les autres ; ses blocs lointains
  restent ceux de sa forme procédurale.
- Le bâtiment reçoit sa **propre définition** dans sa région `.stuff` : un premier
  niveau procédural (ses paramètres d'avant, pour le lointain) et un niveau détaillé
  (les parties du modèle, rangées à la manière du glTF, ses textures PNG ou JPEG telles
  quelles), avec la collision de ce niveau. L'instance renvoie à la définition et passe
  `retouche: true`.
- **Revenir au procédural** ôte la définition et rend au bâtiment sa forme réglable.

## Version 0.27 : retoucher les bâtiments

- Dans un monde matérialisé, l'éditeur (onglet Ajout) choisit un **bâtiment** d'un
  appui : il passe en surbrillance, et le panneau dit d'où il vient (OSM, LiDAR) et s'il
  a été retouché.
- On règle sa forme de toit (plat, deux pans, croupe, appentis) et son sens, la hauteur
  des murs et du toit, son cap, la couleur des murs et de la couverture ; on le déplace
  d'un appui dans la vue (dans sa région), ou on le démolit. Tout s'annule.
- Les retouches se voient tout de suite, collisions comprises, et s'écrivent dans la
  région `.stuff` dès qu'on n'y touche plus : l'instance garde ses autres clés et passe
  `retouche: true` ; un bâtiment démoli reste dans le fichier, `masquee: true`, pour
  qu'une régénération ne le fasse pas revenir. Ses blocs lointains suivent.

## Version 0.26 : le monde matérialisé

- Le lieu réel s'écrit en **régions `.stuff`** (le format du moteur, voir
  [`docs/format-stuff.md`](docs/format-stuff.md)) : 400 régions de 1 km, chacune avec
  son relief à 2 m, son eau, l'occupation du sol, ses routes, ses bâtiments et ponts
  (en instances procédurales, avec leur provenance), ses arbres (en tableau) et sa
  silhouette lointaine. Le jeu lit ensuite ces régions, et plus le `.reel`.
- Après l'import d'un `.reel` (ou au premier lancement avec un lieu déjà importé), la
  matérialisation se fait à part, pendant qu'on joue ; l'onglet Carte de l'éditeur
  montre l'avancement. Une fois finie, la carte passe sur les régions, sans différence
  visible.
- Au lancement, seuls les manifestes et les silhouettes des régions sont lus ; les
  régions se chargent ensuite à mesure qu'on avance.
- Deux outils : `examples/materialiser.rs` matérialise un `.reel` dans un dossier de
  monde, `examples/stuff.rs` affiche le contenu d'un fichier `.stuff`.

## Version 0.25 : les toits

- Les bâtiments du lieu réel ont leur vrai toit : la hauteur et la forme sont mesurées
  par le LiDAR HD de l'IGN (le modèle numérique de hauteur, à 50 cm). Un toit peut
  être plat, à deux pans, en croupe ou en appentis ; il a son sens, son égout et son
  faîtage. Les murs montent jusqu'au toit, pignons compris.
- Un bâtiment absent des mesures (trop récent) garde la hauteur d'OSM, avec deux pans à
  40° s'il est petit.
- L'outil télécharge le MNH par blocs de 250 m là où il y a des bâtiments (environ
  3,5 Go pour Rouen, gardés dans le cache), quatre à la fois, et affiche l'avancement
  (aussi dans `<sortie>.cache/mnh/avancement.txt`).

## Version 0.24 : les bois au loin

- Au-delà des tuiles chargées, les bois du lieu réel ont du volume : une canopée vert
  tacheté à la hauteur des houppiers (12 à 18 m), qui retombe au sol en lisière et dans
  les clairières (les mêmes que celles des arbres de près). Les forêts se détachent sur
  les coteaux jusqu'à l'horizon.
- Près du joueur, les vrais arbres prennent la place ; la canopée ne se voit pas en
  topologie.

## Version 0.23 : la végétation

- Des arbres poussent sur le lieu réel selon l'occupation du sol : serrés dans les bois
  (avec des clairières), épars dans les prés et les parcs, dans les jardins des
  quartiers, rares dans les zones d'activité et les champs.
- Deux essences en facettes : des feuillus au houppier bosselé et des conifères, de
  tailles et de verts variés. Ils ne poussent ni dans l'eau, ni sur les routes, les
  bâtiments ou les ponts.
- Les troncs arrêtent, on passe sous les branches. Toujours les mêmes arbres au même
  endroit. Pas besoin de refaire le `.reel` : les arbres se calculent avec les tuiles.

## Version 0.22 : l'occupation du sol, les bâtiments au loin

- Le sol du lieu réel suit l'occupation d'OpenStreetMap : bois, prés et parcs, champs
  en parcelles de couleurs (blés, labours, cultures vertes), quartiers habités, zones
  d'activité. Ça se voit de près, au loin et sur l'onglet Carte.
- Les grands bâtiments (au moins 100 m² au sol ou 15 m de haut) se voient au-delà des
  tuiles chargées, en blocs à leur hauteur et à leurs couleurs : la ville a une
  silhouette jusqu'à l'horizon. Près du joueur, les vrais bâtiments prennent la place.
- Un `.reel` d'avant se lit encore (sans occupation du sol) ; refaire celui de Rouen avec
  `examples/lieu_reel.rs` l'ajoute.

## Version 0.21 : les ponts

- Le lieu réel porte les ponts d'OpenStreetMap (les voies en `bridge=*`) : la Seine se
  franchit de nouveau, à pied comme sur les autres voies.
- Un tablier de béton de 80 cm, la chaussée dessus à la couleur de la voie, un parapet
  d'un mètre de chaque côté. Les morceaux d'un même pont se mettent bout à bout ; le
  tablier va d'une culée à l'autre, à la hauteur du sol à ses bouts, sans toucher au
  relief ni à l'eau dessous.
- Un `.reel` d'avant se lit encore (sans ponts) ; refaire celui de Rouen avec
  `examples/lieu_reel.rs` les ajoute.

## Version 0.20 : les bâtiments

- Le lieu réel porte les bâtiments d'OpenStreetMap, en volumes à toit plat : maisons,
  immeubles, bâtiments industriels et commerciaux, édifices (églises, écoles…),
  annexes, chacun ses couleurs de murs et de toit.
- Leur hauteur vient d'OSM (`height`, ou les étages à 3 m), sinon de leur genre. Le pied
  des murs descend un mètre sous le point le plus bas du sol : pas de jour sur une pente.
- Ils se chargent avec les tuiles, ne se traversent pas, et la caméra ne passe pas au
  travers. Ils ne se retouchent pas dans l'éditeur, et ne se voient pas en topologie.

## Version 0.19 : les routes

- Le lieu réel porte les routes d'OpenStreetMap (hors ponts et tunnels) : grandes
  routes, routes et rues, voies de service, voies piétonnes, chemins et sentiers, chacun
  sa couleur et sa largeur (celle d'OSM quand elle est donnée).
- Sous la chaussée, le sol est de niveau en travers et lissé le long de la route ; sur
  les bas-côtés (3 m, 1,5 m pour les chemins), il rejoint le relief.
- Un `.reel` d'avant se lit encore (sans routes) ; refaire celui de Rouen avec
  `examples/lieu_reel.rs` les ajoute.

## Version 0.18 : un lieu réel (Rouen)

- `examples/lieu_reel.rs` prépare 20 km autour d'un point de France : le relief de
  l'IGN (RGE ALTI, au pas de 4 m, par la Géoplateforme) et les surfaces d'eau
  d'OpenStreetMap (Overpass). Rouen :

      cargo run --release --example lieu_reel -- Rouen 49.4402 1.0951 rouen.reel

- Dans le jeu : Éditeur › Topologie › CARTE › « Importer un lieu réel… », puis choisir
  le `.reel` (environ 50 Mo). La carte repart de zéro sur ce lieu, le départ au point
  demandé ; il est gardé d'un lancement à l'autre. « Régénérer la carte… » revient à
  une île.
- L'eau (la Seine, et ce qui est à son niveau) a sa surface au niveau médian du relief
  sous elle, son fond creusé jusqu'à 8 m. Les étangs plus hauts restent à sec : le jeu
  n'a qu'un niveau d'eau.
- Sources : relief © IGN (Licence Ouverte 2.0) ; eau © les contributeurs OpenStreetMap
  (ODbL). Un `.reel` est dérivé d'OSM : il reste sous ODbL et n'entre pas dans le dépôt.

## Version 0.17 : voir à 3 km, avec ou sans brouillard

- Au-delà des tuiles chargées, le relief de toute la carte se voit, simplifié (un point
  tous les 64 m) : les montagnes et les côtes de l'île jusqu'à 3 km.
- La caméra tourne autour de la tête du personnage, et non plus de sa taille.
- Réglages › Affichage : « Avec brouillard / Sans brouillard », et le brouillard réglable
  de 30 m à 3 km (règle logarithmique). Sans brouillard, la vue s'arrête net à 3 km.

## Version 0.16 : lever la vue

- En jeu, la vue se lève vers le ciel, jusqu'à environ 60°. Passé l'horizon, la
  caméra descend au ras du sol derrière le personnage et s'en rapproche, garde l'angle
  demandé (le personnage glisse vers le bas de l'image), et le champ s'élargit un peu :
  une perspective en contre-plongée. En édition, la vue reste au-dessus de l'horizon.

## Version 0.15 : le terrassement

- Dans Ajout, l'objet choisi (un bâtiment importé, un bloc…) a une section
  « Terrassement » : sans ou avec sous-sol (profondeur de 1 à 8 m), la marge autour de
  lui, la pente du talus.
- « Terrasser » met la plateforme au niveau du point le plus haut du terrain sous elle
  (en remblai : rien n'est creusé autour), la relie au terrain par un talus, creuse la
  fosse du sous-sol sous l'objet s'il en a un, et y pose l'objet : sur la plateforme, ou
  au fond de la fosse. La terre remuée prend une teinte de terre.
- Une seule annulation défait le tout. Le relief étant une grille de 2 m, les bords de
  la fosse sont en retrait d'un pas sous l'objet : sa pente reste cachée par lui.

## Version 0.14 : l'onglet Carte et la téléportation

- La carte du monde a son onglet dans l'éditeur, « Carte », à côté d'Ajout et de
  Topologie (qui retrouve toute la largeur). À gauche le point visé (position, sol) et
  le joueur ; à droite la carte, sur toute la hauteur.
- Le bouton « Téléporter le joueur ici » le pose au point visé, sur le sol ; pas en eau
  profonde, ni là où le sol n'est pas encore chargé. « Viser le joueur » ramène la vue
  sur lui.

## Version 0.13 : l'érosion

- Le relief de l'île passe par une érosion hydraulique : des centaines de milliers de
  gouttes coulent selon la pente, arrachent la terre où elles vont vite et la déposent
  où elles ralentissent. Les montagnes se ravinent, les vallées se creusent et leurs
  fonds se comblent, la terre emportée finit en deltas.
- Elle se calcule une fois par graine, en quelques secondes et à part du jeu, puis
  s'enregistre (`erosion.bin`, environ 7,5 Mo). « Régénérer la carte… » attend
  qu'elle soit prête ; un monde d'avant la 0.13 s'érode au premier lancement.
- Les couleurs s'en ressentent : fonds de vallée plus verts, ravines rocheuses, terre
  déposée plus brune.
- `examples/erosion.rs` essaie d'autres réglages et en fait une image.

## Version 0.12 : la carte du monde

- En Topologie, le panneau s'élargit et se scinde : les outils à gauche, à droite la
  carte du monde entier sur toute la hauteur. Un pixel par tuile, la couleur du sol
  ombrée par le relief, la mer plus sombre au large.
- Elle montre le point visé et la direction de la vue, la zone chargée autour, et le
  joueur. La toucher ou y glisser y emmène le point visé.
- Les retouches du relief s'y voient (l'image se refait au plus deux fois par seconde),
  celles déjà enregistrées comprises.

## Version 0.11 : une île

- La carte porte une île d'environ 15 km, au contour découpé de caps et de baies, au
  milieu de la mer. Le fond descend vers -35 m au large ; la mer cache le bord de la
  carte.
- Une chaîne de montagnes en crêtes, d'un côté de l'île, monte vers 900 m. Ailleurs,
  des plaines et des collines ; près du rivage, le relief s'efface en plages douces.
- Les couleurs suivent le relief : sable des plages, herbe plus ou moins sèche, lande en
  altitude, roche dans les pentes et vers les sommets, fond de la mer.
- La mer est un plan d'eau jusqu'à l'horizon : houle, reflet du ciel plus fort de biais,
  éclat du soleil, plus transparente vue d'en haut. Le joueur entre dans l'eau jusqu'à
  1,2 m de fond, pas au-delà.
- Une carte d'avant change de relief : « Régénérer la carte… » la remet à neuf, sans
  coutures entre les anciennes tuiles retouchées et les nouvelles.

## Version 0.10 : une carte finie, nue, à régénérer

- La carte fait 20 km de côté : 313 tuiles de 64 m, centrées sur le départ. Au bord, le
  joueur s'arrête comme contre un mur ; au-delà il n'y a rien, que le ciel. L'éditeur
  voit le bord mais ne le passe pas.
- Le générateur ne fait plus que le relief et ses couleurs : la carte est nue, les
  objets se posent à la main. Les tuiles déjà retouchées gardent les leurs.
- « Régénérer la carte… », au bas de l'onglet Topologie, tire une graine au hasard (une
  autre au besoin), dit ce qui sera effacé et attend la confirmation. Tout repart de
  zéro : relief, retouches, objets posés ; le catalogue des modèles reste, et le joueur
  revient au départ.

## Version 0.9 : l'éditeur en pause, la topologie et les poignées

- Le panneau d'édition ouvert, le jeu se met en pause et le personnage disparaît. La
  caméra tourne autour d'un point du sol : la manette le déplace, deux doigts qui
  glissent aussi, et l'on peut s'éloigner jusqu'à 150 m. On voit jusqu'à 1 km, sans
  brouillard. Refermé, le joueur reprend où il était.
- L'éditeur a deux onglets. **Ajout** est l'ancien éditeur. **Topologie** retouche le
  relief au pinceau : Monter, Creuser, Aplanir, Adoucir, Peindre la couleur du sol, avec
  un rayon et une force. Chaque coup de pinceau s'annule, et les tuiles retouchées
  s'enregistrent comme les autres.
  La vue y va jusqu'à la verticale et s'éloigne jusqu'à 2 km ; seul le relief est
  dessiné, avec ses courbes de niveau, dont l'écart suit la hauteur de la vue, et
  l'altitude sous le doigt.
- Dans l'onglet Ajout, l'objet choisi a des poignées : le disque blanc le fait glisser
  sur le sol, les flèches le poussent le long de ses axes ou en hauteur, la pastille
  jaune de l'anneau le tourne (au pas de 5°). Chaque geste est une étape d'annulation.

## Version 0.8.2 : les faces des décors

- Un décor dont les faces regardent vers l'intérieur est retourné à l'import, et le
  message le dit. C'est le cas des fichiers des visualiseurs web qui dessinent tout des
  deux côtés, comme ceux tirés de GTA San Andreas.
- « Retourner ses faces », dans l'éditeur, retourne le modèle à la main, pour tous ses
  objets. Le choix est gardé au catalogue (`"faces"`).
- Un matériau `doubleSided` se voit des deux côtés : feuillages, grilles, vitres.
- Le nom d'un modèle exporté sous « AuxScene » est celui de son fichier.

## Version 0.8.1 : l'affichage

- Les réglages ont un second onglet, Affichage, avec deux curseurs :
  - **la distance d'affichage**, c'est-à-dire jusqu'où les tuiles du monde sont
    chargées : de 64 à 320 m, par tuile de 64 m (128 m à l'origine) ;
  - **le brouillard**, c'est-à-dire là où le lointain a disparu dans le ciel : de 30 m
    jusqu'au bord des tuiles chargées (144 m à l'origine).
- Les tuiles se chargent ou se libèrent au fil des images, sans attendre.
- Les deux réglages sont gardés dans `reglages/affichage.json`.

## Version 0.8 : décors et bâtiments en .glb

- « Ouvrir un .glb » accepte aussi les décors et les bâtiments. Un modèle sans
  squelette entre au catalogue de l'application, sous son identifiant
  (`extras.sage.id` s'il en porte un, sinon `glb/<n>`). L'éditeur s'ouvre aussitôt
  sur AJOUTER, avec le nouveau modèle.
- Le modèle est d'abord posé : son point le plus bas va au sol, et il est centré s'il
  n'a pas son origine dans son emprise. Ses matériaux, couleurs et textures (PNG,
  JPEG) viennent du fichier.
- Chaque objet posé se règle par son cap, son échelle (de ÷ 100 à × 100, pour les
  fichiers en centimètres) et son enfoncement dans le sol.
- La collision suit ses triangles : on marche sur un plancher, on bute contre un mur.
- La tuile ne garde que la référence du modèle, avec la position, le cap et l'échelle.
  Si le fichier manque, l'objet devient une boîte grise d'un mètre, et retrouve sa
  forme dès que le modèle revient. « Remplacer le fichier du modèle » met une nouvelle
  version à la place de l'ancienne, pour tous les objets qui le montrent.
- Limites :
  - sont refusés les maillages compressés (Draco, meshopt) et les textures KTX2 ou
    WebP ;
  - les animations sont ignorées ;
  - au-delà de 50 000 triangles, le modèle est signalé comme lourd à dessiner et à
    heurter.

## Version 0.7 : les manettes de jeu

- Une manette branchée ou appairée en Bluetooth est détectée à sa première touche, et
  fonctionne aussitôt avec une assignation automatique :
  - le stick gauche fait marcher, le droit tourne la vue ;
  - les gâchettes approchent et éloignent la vue ;
  - A choisit l'objet au centre de la vue et X annule, dans l'éditeur ;
  - Select ouvre le moniteur, Y l'éditeur, Start les réglages, et B ferme le panneau.
- Une troisième pastille, l'engrenage, ouvre les réglages. Leur onglet Manette montre :
  - la manette détectée et un témoin en direct de ses sticks et de ses touches ;
  - chaque action et ce qui la commande. On touche une ligne puis on appuie sur la
    touche voulue ou on pousse le stick voulu ; si la touche servait déjà, les deux
    actions s'échangent ;
  - la zone morte des sticks, la vitesse de la vue, et l'inversion verticale.
- Chaque manette garde son profil d'un lancement à l'autre, sous son descripteur
  Android. « Rétablir l'assignation automatique » l'efface.
- Chaque pastille ouvre le panneau sur ses propres onglets.
- Les sticks ne sont plus pris pour des doigts, et B ne quitte plus l'application.

## Version 0.6 : l'édition du monde

- Une seconde pastille, au crayon, ouvre l'éditeur dans le panneau de gauche.
- Touchez un objet dans la vue pour le choisir : il passe en surbrillance. Vous pouvez
  alors :
  - le déplacer d'un appui là où le poser ;
  - le tourner ;
  - changer ses dimensions, son altitude et sa couleur ;
  - le dupliquer ou le supprimer.
- Le catalogue ajoute des blocs, des dalles, des poteaux et des rampes là où l'on
  touche. « Annuler » défait le dernier changement.
- Chaque objet du décor est désormais une entité, sous un identifiant stable.
- Le monde s'enregistre seul dans le stockage de l'application, une tuile modifiée
  entière à la fois :
  - 2 s après un changement ;
  - quand la tuile se décharge ;
  - quand l'application passe en arrière-plan.
- Au lancement suivant, on retrouve le monde tel qu'on l'a laissé, et le joueur là où il
  était.

## Version 0.5 : le rendu

- Les ombres portées : le personnage et le décor portent leur ombre au soleil, sur le
  sol, les dalles et les uns les autres, autour du joueur.
- Les matériaux et les textures du `.glb` sont dessinés : chaque matériau avec sa
  texture, son opacité, et ses parties transparentes. La veste du mannequin a un sergé.
- Le dessin est trié par programme et par texture ; ce qui est transparent vient en
  dernier, du plus loin au plus proche.
- Les tuiles lointaines ont un terrain simplifié : moitié moins de triangles à dessiner.

## Version 0.4.2 : l'inclinaison et le regard

- Le buste penche avec l'élan : en avant quand le personnage part, en arrière quand il
  freine, vers l'intérieur dans les virages.
- La tête regarde ce que vise la caméra : tourner la vue sur le côté ou vers le haut fait
  tourner la tête, dans les limites du cou ; en courant, le regard reste plus près de la
  route ; quand la caméra le regarde de face, il regarde devant lui.
- Le moniteur montre l'inclinaison du buste et le regard.

## Version 0.4.1 : les pieds calés au sol

La dernière couche de l'animation pose les pieds sur le monde :

- un pied posé ne glisse plus : il reste là où il s'est posé jusqu'à ce que l'animation
  le lève ;
- chaque pied se pose sur ce qu'il y a sous lui : au bord d'une dalle, l'un est dessus et
  l'autre au sol, et le bassin descend pour qu'il l'atteigne ;
- sur une pente, le pied en prend l'inclinaison ;
- une marche montée d'un coup est amortie ;
- les genoux plient dans leurs limites, sans jamais se retourner ;
- le moniteur montre chaque pied (tenu, lâché ou levé) et la descente du bassin.

## Version 0.4 : la physique

Le personnage a un corps, une capsule de 1,75 m qui pèse :

- il monte les marches jusqu'à 35 cm sans s'arrêter, et gravit les pentes jusqu'à 45° ;
  au-delà, il ne monte plus et glisse vers le bas ;
- au bord d'une plate-forme, il tombe, à la pesanteur ;
- il bute contre les boîtes, les poteaux (des capsules) et les maillages de collision,
  rangés dans une grille de cases de 2 m, et glisse le long ;
- la campagne en a de chaque sorte : dalles basses à enjamber, poteaux, et une rampe
  qui mène à une plate-forme, une tuile sur deux ;
- des rayons servent au jeu : la caméra se rapproche quand un obstacle la cache, et le
  moniteur montre ce que vise le centre de l'écran, jusqu'à 80 m ;
- le moniteur montre aussi ce qui porte le joueur (le relief ou un objet) et la pente
  sous ses pieds.

## Version 0.3.1 : marche et course, et le personnage qui respire

Le personnage se déplace comme on l'a animé dans Rustcraft :

- marche et course jouées à une même phase, leurs contacts de pied alignés ; la course
  prend le pas entre 1,9 et 2,4 m/s, au-delà seule la cadence change ;
- la cadence suit la vitesse : une foulée par cycle, si bien que la dérive des pieds
  ne dépend pas de la vitesse ;
- à l'arrêt, le mélange d'attente du `.glb` en couches (« Repos vivant » : transfert
  d'appui et respiration) ; sa respiration additive, masquée au haut du corps, reste
  par-dessus la marche et la course ;
- le moniteur montre la part du mouvement et de la course, et ce que joue le
  personnage.

## Version 0.3 : le monde en tuiles

Le joueur marche dans une campagne sans fin, en tuiles de 64 m :

- les 25 tuiles autour du joueur sont chargées, une par image pour éviter les
  à-coups, et déchargées au-delà de trois tuiles ;
- les positions sont relatives à la tuile du joueur, qui sert d'origine : le monde se
  décale d'une tuile quand il en change, et reste précis à n'importe quelle distance ;
- chaque tuile a son identifiant (`tuile/i/j`), un relief en collines et des blocs posés
  dessus ; deux tuiles voisines s'accordent exactement sur leur bord ;
- le joueur suit le relief ; le lointain se fond dans le ciel ;
- le moniteur montre les tuiles chargées, le temps de génération, la tuile et la
  position absolue du joueur.

## Version 0.2.1 : le moniteur

La pastille en haut à gauche déploie le panneau latéral (en haut, en portrait), qui
montre en direct :

- les images : cadence, intervalle et pire intervalle, courbe des 120 dernières
  images (repères à 60 et 30 images par seconde), temps du jeu, de l'envoi au rendu
  et de l'échange des tampons ;
- la simulation : pas fixes par seconde et par image ;
- le monde : entités, objets dessinés, triangles, maillages et leur poids ;
- le joueur : position, cap, vitesse, part de la marche dans l'animation.

La 3D se dessine dans la place que laisse le panneau, redimensionnable à sa poignée.

## Version 0.2 : le socle du moteur

Le jeu de la 0.1 tourne maintenant sur un moteur à part entière :

- un monde d'entités désignées par des poignées, avec leurs composants (lieu, rendu,
  squelette, obstacles, mobilité) ;
- une simulation à pas fixes de 1/60 s, dont les images sont interpolées : le jeu se
  comporte de la même façon à toutes les cadences ;
- des maillages enregistrés sous leur identifiant et versionnés : seul ce qui change
  repart au processeur graphique ;
- une scène extraite du monde à chaque image, seule chose que lit le moteur de rendu.

Le socle se poursuit par le monde en tuiles, la physique et le rendu ; voir
[`docs/architecture.md`](docs/architecture.md).

## Version 0.1 : un personnage jouable

- Ouvre un `.glb` exporté par Rustcraft pour S.A.G.E. (armature, peau, animations)
  et le garde pour le prochain lancement ; un mannequin intégré sert en attendant.
- La peau est déformée sur le processeur graphique (48 os au plus).
- Les animations de repos et de marche se mêlent selon la vitesse, et leur rythme
  suit le pas. Elles sont reconnues par leur nom (« repos », « idle »… ;
  « marche », « walk »…). Une animation seule sert de marche.
- Manette tactile sur la gauche de l'écran ; ailleurs, un doigt tourne la caméra et
  deux doigts l'approchent.
- Le personnage marche sur un sol quadrillé, parmi des blocs qu'il ne traverse pas.

Pour les appareils ARM 64 bits, sous Android 8.0 ou plus récent, avec OpenGL ES 3.

## Organisation

- `src/moteur/` : le moteur, sans Android ni OpenGL : entités et composants
  (`entites`), temps à pas fixe (`horloge`), maillages (`ressources`), scène à dessiner
  (`scene`), tuiles et origine flottante (`tuiles`), corps, obstacles et rayons
  (`physique`), allures (`locomotion`), posture (`posture`), pieds calés (`pieds`), systèmes de marche et d'animation
  (`systemes`) ;
- `src/moteur/objets.rs`, `src/moteur/depot.rs` : les objets du décor et leur
  catalogue ; le dépôt du monde en JSON ;
- `src/campagne.rs` : la source de tuiles générée ;
- `src/editeur.rs` : l'éditeur du monde, dans le panneau de gauche (onglets Ajout et
  Topologie, poignées de l'objet choisi) ;
- `src/terrain.rs` : le pinceau qui retouche le relief des tuiles ;
- `src/manette.rs`, `src/reglages.rs` : les manettes de jeu (actions, profils,
  assignation) et l'onglet Manette des réglages ;
- `src/image.rs` : les textures PNG et JPEG, décodées ;
- `src/jeu.rs` : le jeu bâti sur le moteur (manette, caméra, décor, interface,
  panneau) ;
- `src/moniteur.rs` : les mesures du moniteur ;
- `src/personnage.rs` : le personnage relu d'un `.glb`, mis à plat pour le
  processeur graphique ; le mannequin intégré et ses animations ;
- `src/renderer.rs`, `src/gles.rs` : le rendu OpenGL ES de la scène (carte d'ombre,
  sol, objets statiques ou à peau, matériaux et textures, interface) ;
- `src/android.rs`, `java/` : l'activité native, le sélecteur de fichiers ;
- `src/ui.rs`, `widgets.rs`, `shell.rs`, `draw.rs`, `text.rs` : l'interface tactile et
  ses panneaux, reprise de
  [Rustine](https://github.com/s1pierro/Rustine).

Le modèle (maillages, armatures, peaux, animations, lecture et écriture du `.glb`)
vient de la crate [`gda`](https://github.com/s1pierro/gda-modele), partagée avec Rustcraft. Le format est décrit
dans `rustcraft/docs/format-gda.md`.

## Construire

La crate `gda` doit se trouver à côté de `sage`, dans un dossier nommé `gda` :

    git clone https://github.com/s1pierro/sage
    git clone https://github.com/s1pierro/gda-modele gda

Puis, dans Termux :

    bash build.sh                              # → dist/sage.apk
    cargo test                                 # le jeu sans écran
    cargo run --release --example apercu -- <dossier> [personnage.glb]
                                               # images PPM rendues sur le GPU

## Licence

[PolyForm Noncommercial 1.0.0](LICENSE.md).
