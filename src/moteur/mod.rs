//! Le moteur : le monde d'entités, le temps à pas fixe, les ressources à dessiner et la
//! scène que lit le moteur de rendu. Rien ici ne connaît Android ni OpenGL.
//!
//! Voir `docs/architecture.md`.

pub mod entites;
pub mod depot;
pub mod horloge;
pub mod locomotion;
pub mod objets;
pub mod physique;
pub mod pieds;
pub mod posture;
pub mod ressources;
pub mod scene;
pub mod systemes;
pub mod tuiles;

pub use entites::{Aabb, Entite, Lieu, Mobile, Monde, Rendu, Squelette, Stock};
pub use horloge::{Horloge, PAS_FIXE};
pub use physique::{Corps, Forme, Impact, Triangles};
pub use ressources::{Alpha, Format, MaillageId, Maillages, Materiau, Partie, TextureId, Textures};
pub use scene::{Camera, Dessin, Scene, Soleil};
pub use objets::{Genre, Objet};
pub use tuiles::{Contenu, Element, Relief, Source, Tuiles, TAILLE};
