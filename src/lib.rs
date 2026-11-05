//! S.A.G.E. : le moteur de jeu, en application Android native, en Rust.
//!
//! - `gles`, `renderer`, `android` : ce qui touche à OpenGL ES et à Android ;
//! - `draw`, `text`, `ui`, `widgets`, `shell` : l'interface et ses panneaux, sans OpenGL ;
//! - `moniteur` : cadence, temps par image, état du moteur ;
//! - `moteur` : le monde d'entités, le temps à pas fixe, les ressources, la scène ;
//! - `jeu` : le jeu bâti sur le moteur (commandes, caméra, interface), sans OpenGL ;
//! - `editeur` : l'édition du monde et de ses objets, dans le panneau de gauche ;
//! - `manette`, `reglages` : les manettes de jeu, leurs profils, et l'onglet qui les règle ;
//! - `terrain` : le pinceau qui retouche le relief des tuiles ;
//! - `campagne` : la source de tuiles générée (relief, objets) ;
//! - `catalogue` : les modèles importés d'un `.glb` (décors, bâtiments) ;
//! - `personnage` : le personnage relu d'un .glb, prêt pour le processeur graphique ;
//! - `image` : les textures PNG et JPEG, décodées.
//!
//! Le modèle (maillages, armatures, peaux, animations, format d'échange) vient de la
//! crate `gda`, partagée avec Rustcraft.

pub mod android;
pub mod campagne;
pub mod catalogue;
pub mod draw;
pub mod editeur;
pub mod gles;
pub mod image;
pub mod jeu;
pub mod manette;
pub mod math;
pub mod moniteur;
pub mod moteur;
pub mod personnage;
pub mod reglages;
pub mod renderer;
pub mod shell;
pub mod terrain;
pub mod text;
pub mod ui;
pub mod widgets;
