package app.valanium;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;
import java.io.File;
import java.io.FileNotFoundException;

/** Grants the Android installer read access to one downloaded APK, never account files. */
public final class UpdateProvider extends ContentProvider {
    @Override public boolean onCreate(){return true;}
    private File resolve(Uri uri) throws FileNotFoundException {
        String name=uri.getLastPathSegment();
        if(name==null||!name.matches("Valanium-[0-9]{1,6}\\.[0-9]{1,6}\\.[0-9]{1,6}\\.apk")
                ||!uri.getPath().equals("/"+name))throw new FileNotFoundException("Invalid update URI");
        try {
            File root=new File(getContext().getCacheDir(),"updates").getCanonicalFile();
            File file=new File(root,name).getCanonicalFile();
            if(!root.equals(file.getParentFile())||!file.isFile())throw new FileNotFoundException("Update unavailable");
            return file;
        }catch(java.io.IOException error){throw new FileNotFoundException("Update unavailable");}
    }
    @Override public ParcelFileDescriptor openFile(Uri uri,String mode)throws FileNotFoundException{
        if(!"r".equals(mode))throw new FileNotFoundException("Read only");
        return ParcelFileDescriptor.open(resolve(uri),ParcelFileDescriptor.MODE_READ_ONLY);
    }
    @Override public String getType(Uri uri){return "application/vnd.android.package-archive";}
    @Override public Cursor query(Uri uri,String[] projection,String selection,String[] args,String order){
        try{File file=resolve(uri);MatrixCursor cursor=new MatrixCursor(new String[]{OpenableColumns.DISPLAY_NAME,OpenableColumns.SIZE});
            cursor.addRow(new Object[]{file.getName(),file.length()});return cursor;
        }catch(FileNotFoundException error){return null;}
    }
    @Override public Uri insert(Uri uri,ContentValues values){throw new UnsupportedOperationException();}
    @Override public int update(Uri uri,ContentValues values,String selection,String[] args){throw new UnsupportedOperationException();}
    @Override public int delete(Uri uri,String selection,String[] args){throw new UnsupportedOperationException();}
}
