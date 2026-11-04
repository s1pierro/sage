package fr.s1pierro.sage;

import android.app.NativeActivity;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.widget.Toast;
import android.net.Uri;

/**
 * NativeActivity ne transmet pas onActivityResult au code natif : cette classe ouvre
 * le sélecteur de fichiers d'Android et passe le document choisi à Rust.
 */
public class Activite extends NativeActivity {
    /** Appelée depuis le fil de rendu. La demande revient avec le résultat. */
    public void choisir(final int demande) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                Intent i = new Intent(Intent.ACTION_OPEN_DOCUMENT);
                i.addCategory(Intent.CATEGORY_OPENABLE);
                // .glb n'a pas de type reconnu partout
                i.setType("*/*");
                try {
                    startActivityForResult(i, demande);
                } catch (ActivityNotFoundException e) {
                    Toast.makeText(Activite.this, "Aucun sélecteur de fichiers", Toast.LENGTH_LONG).show();
                }
            }
        });
    }

    @Override
    protected void onActivityResult(int demande, int resultat, Intent donnees) {
        super.onActivityResult(demande, resultat, donnees);
        if (resultat == RESULT_OK && donnees != null && donnees.getData() != null) {
            recu(donnees.getData(), demande, 0);
        }
    }

    /** Enregistrée par ANativeActivity_onCreate. */
    private native void recu(Uri uri, int demande, int reste);
}
