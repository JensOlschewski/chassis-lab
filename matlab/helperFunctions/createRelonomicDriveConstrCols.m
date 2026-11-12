% #########
%  Helper function to create additional ContraintMatrix columns by relonomic drive
% #########
function jointStructOut = createRelonomicDriveConstrCols(relonomicDriveStruct,jointStructOut)
    namesDrives = fieldnames(relonomicDriveStruct);
    
    for i = 1:length(namesDrives)
        driveName = namesDrives{i};
        driveData = relonomicDriveStruct.(driveName);
        jointID = driveData.jointID;
        signalCell = driveData.signal;

        % Check which Toplogy Type of the joint 
        jointTopoType = driveData.jointTopology;

        if strcmp(jointStructOut(jointID).topology,jointTopoType)
            % TODO: Implementiere Logik für primäre Gelenke Hj Matrix

            signalArray = ~strcmp(signalCell, '~'); 
            numSignals = sum(signalArray);
            reloDriveConstrCols = zeros(numSignals, 6); 
            [reloDriveConstrCols(:, find(signalArray))] = deal(1);

            % Erstelle die Rotationsmatrixen
            R_i = createBlkDiag(jointStructOut(jointID).i.eBasis,jointStructOut(jointID).i.eBasis);
            R_j = createBlkDiag(jointStructOut(jointID).j.eBasis,jointStructOut(jointID).j.eBasis);

            for j = 1:numSignals
                vec_unrotated = reloDriveConstrCols(j, :).';
                vec_rotated_i = R_i * vec_unrotated;
                vec_rotated_j = R_j * -vec_unrotated;
                
                % Speichern der rotierten Vektoren
                jointStructOut(jointID).i.reloDriveConstrCols(j, :) = vec_rotated_i.';
                jointStructOut(jointID).j.reloDriveConstrCols(j, :) = vec_rotated_j.';
            end
        else
            continue
        end
    end
end