function bushStruct = fun_initBushStruct(jsonBushStruct, bodiesStruct, jointStruct)
    % ##################################################################################
    % Function: fun_initBushStruct
    % Description: Initializes forceAdd structures based on the provided JSON structure
    %              and the current bodies configuration. This includes setting up
    %              type-specific parameters and calculating initial deflections and load.
    %
    % Inputs:
    %   jsonForceAddStruct - Structure containing force connector's configuration and 
    %                        type information.
    %   bodiesStruct       - Structure containing information about the existing bodies.
    %
    % Output:
    %   forceAddStruct - Initialized structure array for each force connection with
    %                    type-specific details.
    % ##################################################################################

    % Initalise empty struct 
    bushStruct = struct();

    % Retrieve names of bushings from jsonBushStruct
    bushNames = fieldnames(jsonBushStruct);

    for i = 1:numel(bushNames)
        bushName = bushNames{i};
        currentBush = jsonBushStruct.(bushName);
        
        % Get basic information -> also when not active
        bushStruct(i).name = bushName;
        bushStruct(i).active = logical(currentBush.active);
        bushStruct(i).conType = 'bushing';
        bushStruct(i).jointID = currentBush.jointID;
        
        % Process only if currentBush is active
        if bushStruct(i).active

            % Extract bushing topology 
            bushStruct(i).topology = currentBush.bushTopology;

            % Determine sides and create connector parts
            [iSide, jSide] = assignSidesToConStrut(currentBush, bodiesStruct);
            bushStruct(i).i = createConPart(currentBush.i, bodiesStruct, iSide);
            bushStruct(i).j = createConPart(currentBush.j, bodiesStruct, jSide);

            % Determine bodyOrder isJGreaterThanI ?
            bushStruct(i).isJGreaterThanI = bushStruct(i).i.bodyID < bushStruct(i).j.bodyID;
            
            % Determine preload
            if isfield(currentBush,'preload')
                bushStruct(i).loadStruct.preload = currentBush.preload;          
            else
                bushStruct(i).loadStruct.preload = zeros(6,1);
            end

            % Determine freeLength
            if isfield(currentBush,'free_length')
                bushStruct(i).loadStruct.freeLength = currentBush.free_lenght;          
            else
                bushStruct(i).loadStruct.freeLength = zeros(6,1);
            end

            
            % Retrieve stiffness characteristic information and determine charStruct 
            bushStruct(i).charStruct = createCharStruct(currentBush.stiffness, 'bushing');
            
            % Calculate initial deflection and kinematic motion
            bushStruct(i).loadStruct.currentDeflection = calculateConMotion(...
            bushStruct(i).i.point, bushStruct(i).j.point,... 
            bushStruct(i).i.eBasis, bushStruct(i).j.eBasis,... 
            'bushing',bushStruct(i).loadStruct.freeLength);
            
            bushStruct(i).loadStruct.currentKinMotion = zeros(6,1);

            % Determine jointType and assign NullSpaceMatrix
            bushStruct(i).jointType = jointStruct(bushStruct(i).jointID).type;
            bushStruct(i).charStruct.NullSpaceMatrix = jointStruct(bushStruct(i).jointID).eG;

            % Assign pointJoint eBasisJoint
            bushStruct(i).i.pointJoint = jointStruct(bushStruct(i).jointID).i.point;
            bushStruct(i).j.pointJoint = jointStruct(bushStruct(i).jointID).j.point;
            bushStruct(i).i.eBasisJoint = jointStruct(bushStruct(i).jointID).i.eBasis;
            bushStruct(i).j.eBasisJoint = jointStruct(bushStruct(i).jointID).j.eBasis;     

            % Create and assign local bush matrices
            [kMatrixLok, cMatrixLok, cMatrixLokRange, kMatrixLokNullspace] = createLocalCKMatrices(bushStruct(i),'bushing');
            bushStruct(i).charStruct.kMatrixLok = kMatrixLok;
            bushStruct(i).charStruct.cMatrixLok = cMatrixLok;
            bushStruct(i).charStruct.cMatrixLokRange = cMatrixLokRange;
            bushStruct(i).charStruct.kMatrixLokNullspace = kMatrixLokNullspace;
            
            % Determine Current Load
            bushStruct(i).loadStruct.currentLoad =  kMatrixLok * bushStruct(i).loadStruct.currentDeflection;
            
            % Check if any bushing is preloaded
            bushStruct(i).loadStruct.isPreload = any(bushStruct(i).loadStruct.currentLoad ~= 0); 

        end
    end
end
