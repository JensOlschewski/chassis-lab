function bodiesStructOut = fun_initBodyStruct(bodiesStrucIn, hardpointStruct)
%init_bodyStruct Return a struct containing all  contents of file as plain text
%   TEXT = FILEREAD(FILENAME) returns the TEXT contents of the file FILENAME.
%   TEXT = FILEREAD(___,"Encoding",ENC) uses the encoding ENC to convert the text.
%
%  Note: FILEREAD interprets all files as plain-text. 
%
%   See also READLINES, WRITELINES, SPLITLINES, FREAD, UIIMPORT

% Written by Jens Olschewski -> jensolschewski@me.com 

    bodyNames = fieldnames(bodiesStrucIn);
    numBodies = length(bodyNames);
    bodiesStructOut = struct();

    for i = 1:numBodies
        bodyName = bodyNames{i};
        bodyID = bodiesStrucIn.(bodyName).bodyID;
        
        bodiesStructOut(bodyID).name = bodyNames{i};
        bodiesStructOut(bodyID).bodyID = bodyID; 
        bodiesStructOut(bodyID).side = bodiesStrucIn.(bodyName).side;             
        bodiesStructOut(bodyID).cm = bodiesStrucIn.(bodyName).CM; 
        bodiesStructOut(bodyID).points = CreatePointsOnBody(bodiesStrucIn, bodyName, bodyID, hardpointStruct);

    end
    
    bodiesStructOut = addGroundBody(bodiesStructOut, hardpointStruct);
end


%% ######################
% Helper functions ######
% #######################

% #########
%  Helper function for Ground Body 
% #########
function updatedbodiesStructOut = addGroundBody(bodiesStructOut, hardpointStruct)
    pointNames = fieldnames(hardpointStruct);
    bodiesStructOut(end+1).name = 'B0_GROUND';
    bodiesStructOut(end).bodyID = 0;
    bodiesStructOut(end).points.names = pointNames;

    % Definiere die Spiegelmatrix für die Spiegelung an der XZ-Ebene
    MirrorMatrix = [1 0 0; 0 -1 0; 0 0 1];

    for i = 1:length(pointNames)
        originalPointName = pointNames{i};
        pointName_ls = [lower(originalPointName) '0_ls']; % Linker Punkt
        pointName_rs = [lower(originalPointName) '0_rs']; % Rechter Punkt

        % Füge den linken Punkt hinzu
        bodiesStructOut(end).points.(pointName_ls) = hardpointStruct.(originalPointName);

        % Füge den gespiegelten rechten Punkt hinzu
        bodiesStructOut(end).points.(pointName_rs) = MirrorMatrix * hardpointStruct.(originalPointName);
    end

    updatedbodiesStructOut = bodiesStructOut;
end

% #########
%  Helper function for point on body initalisation 
% #########
function bodyStruct = CreatePointsOnBody(bodiesStrucIn, bodyName, bodyID, hardpointStruct)
    points = bodiesStrucIn.(bodyName).Points_on_Body;
    side = bodiesStrucIn.(bodyName).side;  % Hier holen wir die Seite des Körpers
    bodyStruct = struct();
    
    % Definiere die Spiegelmatrix für die Spiegelung an der XZ-Ebene
    MirrorMatrix = [1 0 0; 0 -1 0; 0 0 1];

    for i = 1:length(points)
        originalPointName = points{i};
        if isfield(hardpointStruct, originalPointName)
            pointName = [lower(originalPointName) num2str(bodyID)]; 

            % Prüfe die Seite und wende die Spiegelmatrix an, falls notwendig
            if strcmp(side, 'ls')
                bodyStruct.(pointName) = hardpointStruct.(originalPointName);
            elseif strcmp(side, 'rs')
                bodyStruct.(pointName) = MirrorMatrix * hardpointStruct.(originalPointName);
            else
                error('Unknown side: %s', side);
            end
        end
    end
end