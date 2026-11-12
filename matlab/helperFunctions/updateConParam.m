function updatedConStruct = updateConParam(oldConStruct, vHat, vCon, h, uMode)
    % ##################################################################################
    % updateConParam Updates connector parameters based on velocity and time step
    %
    % This function iterates through each connector in the oldConStruct and updates its
    % parameters (eBasis, bodyCM, point, s) based on the velocity vectors vHat and vCon,
    % and the time step h. The updates account for both translation and rotation.
    %
    % The function handles different connector types ('joint', 'bushing', 'spring') and
    % updates parameters according to their specific requirements. For 'elastic' connectors
    % and when uMode is set to 'elastic', it calculates updated velocities using
    % 'calculateConInducedVelocity' function.
        % Inputs:
    %   oldConStruct - Structure array containing original connector parameters.
    %   vHat         - 6*N x 1 vector of body velocities (first 3*N for translation, next 3*N for rotation).
    %   vCon         - 6*N x 1 vector of connector velocities (used when uMode is 'elastic' and conType is 'elastic').
    %   h            - Time step for the update.
    %   uMode        - Update mode, 'elastic' or other modes.
    %
    % Output:
    %  updatedConStruct - Structure array with updated connector parameters.
    % ##################################################################################
    

    % Initialize updatedConStruct and side 
    updatedConStruct = oldConStruct;
    sides = {'i', 'j'};

    for i = 1:numel(oldConStruct)
        currentConStruct = oldConStruct(i);
        conType = currentConStruct.conType;
        
        % Skip inactive connectors except for joints, because they are always active and don't have this field
        if ~strcmp(conType, 'joint') && currentConStruct.active == 0
            continue

        elseif strcmp(conType, 'joint') 
            jointMode = currentConStruct.jointMode;
            
        end
        
        % Calculate viUpdate and vjUpdate if needed
        if strcmp(uMode, 'elastic')
            if isempty(vCon)
                error('uMode is elastic but no elastic velocity vector is provided');
            elseif (strcmp(conType, 'joint') && strcmp(uMode, 'elastic') && strcmp(jointMode, 'elastic')) || ...
                   (strcmp(conType, 'bushing') && strcmp(uMode, 'elastic'))
                [viUpdate, vjUpdate] = calculateConInducedVelocity(currentConStruct, conType, vHat, vCon);
            end
        end
            
        for k = 1:numel(sides)
            side = sides{k};
            currentConPartStruct = currentConStruct.(side);
            bodyID = currentConPartStruct.bodyID;

            % No update for ground 
            if bodyID == 0
                continue
            end

            oldCM = currentConPartStruct.bodyCM;
            oldSVec = currentConPartStruct.s;
            if isfield(currentConPartStruct, 'eBasis')
                oldEbasis = currentConPartStruct.eBasis;
            end
            idxVHat = (bodyID * 6 - 5):(bodyID * 6);
            vUpdate = vHat(idxVHat);

            switch conType 

                case 'joint' 
                    if strcmp(currentConStruct.type, 'Symmetriebedingung')
                        % Subcase symmetrie for joint
                        [newCM, newPoint, newEbasis, newSVec] = updateWithVelo(oldCM, oldSVec, oldEbasis, vUpdate, h, 'position');
                    
                    elseif strcmp(uMode, 'kinematic') || ...
                           strcmp(uMode, 'elastic') && strcmp(jointMode, 'kinematic')
                        % Subcase kinematic update
                        [newCM, newPoint, newEbasis, newSVec] = updateWithVelo(oldCM, oldSVec, oldEbasis, vUpdate, h, 'all');
                    
                    elseif strcmp(uMode, 'elastic') && strcmp(jointMode, 'elastic')
                        % Subcase elastic update
                        [newCM, newPoint, newEbasis, newSVec] = updateElasticCase(side, viUpdate, vjUpdate, h, oldCM, oldSVec, oldEbasis);
                        
                    end

                case 'bushing'
                    if strcmp(uMode, 'elastic') 
                        [newCM, newPoint, newEbasis, newSVec] = updateElasticCase(side, viUpdate, vjUpdate, h, oldCM, oldSVec, oldEbasis);
                    elseif strcmp(uMode, 'kinematic')
                        % Subcase kinematic update
                        [newCM, newPoint, newEbasis, newSVec] = updateWithVelo(oldCM, oldSVec, oldEbasis, vUpdate, h, 'all');

                    end

                case 'spring' 
                        % Update spring case position only
                        [newCM, newPoint, newEbasis, newSVec] = updateWithVelo (oldCM, oldSVec, [], vUpdate, h,'position');

                otherwise 
                    error('Unknown Connector Type defined: %s', conType);
                    
            end
                        
            % Assign updated parameters
            updatedConStruct(i).(side).eBasis = newEbasis;
            updatedConStruct(i).(side).bodyCM = newCM;
            updatedConStruct(i).(side).s = newSVec;
            updatedConStruct(i).(side).point = newPoint;
        end
    end
end

% #################
% Helper Functions
% #################
    
% ----------------------------------------------------------------
function [newCM, newPoint, newEbasis, newSVec] = updateElasticCase(side, viUpdate, vjUpdate, h, oldCM, oldSVec, oldEbasis)
    % Subcase elastic update
    if strcmp(side, 'i')
        vUpdate = viUpdate;
    elseif strcmp(side, 'j')
        vUpdate = vjUpdate;
    end
    [newCM, newPoint, newEbasis, newSVec] = updateWithVelo(oldCM, oldSVec, oldEbasis, vUpdate, h, 'all');
end
% ----------------------------------------------------------------

% ----------------------------------------------------------------
function [viConUpdate, vjConUpdate] = calculateConInducedVelocity(currentConStruct, conType, vHat, vCon)

    % Extract bodyID for i and j side
    ibodyID = currentConStruct.i.bodyID;
    jbodyID = currentConStruct.j.bodyID;

    vjHat = zeros(6, 1);
    viHat = zeros(6, 1);
    
    % Extract transformation matrices
    Ti = currentConStruct.Ti;    
    Tj = currentConStruct.Tj;    
    Ti_inv = currentConStruct.Ti_inv;    
    Tj_inv = currentConStruct.Tj_inv;
    
    if strcmp(currentConStruct.topology,'primary') || strcmp(currentConStruct.topology,'secondary') && strcmp(conType, 'joint')

        viCon = zeros(6, 1);
        vjCon = zeros(6, 1);
        
        if jbodyID ~= 0
            vjHat = vHat((jbodyID * 6 - 5):(jbodyID * 6));
            vjCon = vCon((jbodyID * 6 - 5):(jbodyID * 6));
        end

        if ibodyID ~= 0
            viHat = vHat((ibodyID * 6 - 5):(ibodyID * 6));
            viCon = vCon((ibodyID * 6 - 5):(ibodyID * 6));
        end

        % Extract relative connector motion from i to j
        vijConRelative = (Tj_inv * vjCon - Ti_inv * viCon);
    end
    
    if strcmp(currentConStruct.topology,'secondary') && strcmp(conType, 'bushing')
        vijConRelative = vCon(currentConStruct.jointID * 6 - 5:currentConStruct.jointID * 6); 
    end


    switch conType
        case 'bushing'
            % Calculate for bushing case 
            % viConUpdate = viHat;
            % vjConUpdate = Tj * (Ti_inv * viHat + vijConRelative);      

            vjConUpdate = vjHat; 
            viConUpdate = Ti *  (Tj_inv * vjHat - vijConRelative);
            
            
        case 'joint'
            % Calculate for joint case
            %vjConUpdate = vjHat; % !!!! 
            %viConUpdate = Ti *  (Tj_inv * vjHat - vijConRelative);

            viConUpdate = viHat;
            vjConUpdate = Tj * (Ti_inv * viHat + vijConRelative);                
    end
end

% ----------------------------------------------------------------
function [newCM, newConPoint, newEbasis, newSVec] = updateWithVelo(oldCM, oldSVec, oldEbasis, vUpdate, h, uMode)
    % Calculate the exponential rotation matrix
    R = createRotFromAngVel(vUpdate(4:6), h, 'small');       

    % Update ConnectorPoint CM and sVec 
    newConPoint = (oldCM + vUpdate(1:3) * h) + R * oldSVec;    
    newCM = oldCM + vUpdate(1:3) * h;
    newSVec = newConPoint - newCM;

    switch uMode
        case 'position'
            if isempty(oldEbasis) 
            newEbasis = [];
            else
                newEbasis = oldEbasis;
            end
        case 'all'
            newEbasis = R * oldEbasis;
    end

end