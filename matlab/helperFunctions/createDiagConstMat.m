function [Gpi,Gpj,Gsi,Gsj] = createDiagConstMat (jointPrimStruct,jointSecStruct)
        % createDiagConstMat erstellt diagonale Constraint-Matrizen für primäre und sekundäre Gelenke
        %
        % Diese Funktion generiert Block-Diagonalmatrizen für primäre (Gpi, Gpj) und sekundäre (Gsi, Gsj) Gelenke.
        % Für jede Gelenkstruktur (primär und sekundär) werden zwei Matrizen erstellt, die die geometrischen
        % Constraints der Gelenke in einem mechanischen System darstellen.
        %
        % Eingabe:
        %   jointPrimStruct - Struktur mit Daten zu den primären Gelenken
        %   jointSecStruct - Struktur mit Daten zu den sekundären Gelenken
        %
        % Ausgabe:
        %   Gpi - Block-Diagonalmatrix der Constraints für primäre Gelenke (Gi-Komponente)
        %   Gpj - Block-Diagonalmatrix der Constraints für primäre Gelenke (Gj-Komponente)
        %   Gsi - Block-Diagonalmatrix der Constraints für sekundäre Gelenke (Gi-Komponente)
        %   Gsj - Block-Diagonalmatrix der Constraints für sekundäre Gelenke (Gj-Komponente)
        % ...
        % Construct Gp matrix (Global constraint Matrix of the primary joints)
        Gpi = createBlkDiag(jointPrimStruct(:).Gi);
        Gpj = createBlkDiag(jointPrimStruct(:).Gj); 

        Gsi = createBlkDiag(jointSecStruct(:).Gi);
        Gsj = createBlkDiag(jointSecStruct(:).Gj); 
end