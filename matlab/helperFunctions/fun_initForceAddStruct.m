function forceAddStruct = fun_initForceAddStruct(jsonForceAddStruct, bodiesStruct)
    % ##################################################################################
    % Function: fun_initForceAddStruct
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
    forceAddStruct = struct();
    
    % Retrieve names of force connections from jsonForceAddStruct
    forceAddNames = fieldnames(jsonForceAddStruct);

    % Iterate through each forceAdd name
    for i = 1:length(forceAddNames)

        forceAddStruct(i).topology = 'additional';

        forceAddName = forceAddNames{i};
        currentForceAdd = jsonForceAddStruct.(forceAddName);

        % Populate forceAddStruct with basic information -> also when not active
        forceAddStruct(i).name = forceAddName;
        forceAddStruct(i).active = logical(currentForceAdd.active);

        % Retrieve and set the type
        currentConType = currentForceAdd.type;
        forceAddStruct(i).conType = currentConType;

        % Process only if the forceAdd is active
        if forceAddStruct(i).active

            % Retrieve stiffness characteristic information and determine charStruct 
            forceAddStruct(i).charStruct = createCharStruct(currentForceAdd.stiffness, currentConType);
            
            % Handle different connector types
            switch currentConType
                case "spring"

                    % Assign orientMethod to spring
                    currentForceAdd.i.orientation.method = 'line-ji';
                    currentForceAdd.j.orientation.method = 'line-ji';

                    % Determine sides and create connector parts
                    [iSide, jSide] = assignSidesToConStrut(currentForceAdd, bodiesStruct);
                    forceAddStruct(i).i = createConPart(currentForceAdd.i, bodiesStruct, iSide);
                    forceAddStruct(i).j = createConPart(currentForceAdd.j, bodiesStruct, jSide);
                    
                    % Retrieve free_lenght and preload information from json 
                    if isfield(currentForceAdd, 'free_length') && ~isempty(currentForceAdd.free_length)
                        freeLength = currentForceAdd.free_length;
                    else
                        freeLength = norm(forceAddStruct(i).i.point - forceAddStruct(i).j.point);
                    end
                    preload = currentForceAdd.preload ;
                    deltaPos = forceAddStruct(i).i.point - forceAddStruct(i).j.point;
                    
                    
                    % Calculate initial deflection and preload
                    forceAddStruct(i).loadStruct.freeLength = freeLength;
                    forceAddStruct(i).loadStruct.preload = preload;
                    forceAddStruct(i).loadStruct.springDirection = deltaPos / norm(deltaPos);
                    forceAddStruct(i).loadStruct.currentDeflection = calculateConMotion(...
                        forceAddStruct(i).i.point, forceAddStruct(i).j.point,...
                        [], [],...
                        'spring', freeLength);
                    
                    % If nonlinear extract stiffness and load based on currentDeflection
                    if ~forceAddStruct(i).charStruct.isLinear 
                        [kValue,currentLoad] = extractStiffness(forceAddStruct(i).charStruct, forceAddStruct(i).loadStruct.currentDeflection);
                        forceAddStruct(i).charStruct.k.kValue = kValue;
                        forceAddStruct(i).loadStruct.currentLoad = currentLoad + forceAddStruct(i).loadStruct.preload;

                    % If linear, calculate current load based on kValue and currentDeflection
                    elseif forceAddStruct(i).charStruct.isLinear 
                        forceAddStruct(i).loadStruct.currentLoad = -forceAddStruct(i).charStruct.k.kValue * forceAddStruct(i).loadStruct.currentDeflection + preload ;
                    end

                    % Create and assign local bush matrices
                    [kMatrixLok, cMatrixLok, cMatrixLokRange, kMatrixLokNullspace] = createLocalCKMatrices(forceAddStruct(i),'spring');
                    forceAddStruct(i).charStruct.kMatrixLok = kMatrixLok;
                    forceAddStruct(i).charStruct.cMatrixLok = cMatrixLok;
                    forceAddStruct(i).charStruct.cMatrixLokRange = cMatrixLokRange;
                    forceAddStruct(i).charStruct.kMatrixLokNullspace = kMatrixLokNullspace;


                    % Determine isPreload for preload loop
                    forceAddStruct(i).loadStruct.isPreload = any(forceAddStruct(i).loadStruct.currentLoad ~= 0);

                % Add additional cases for other topologies here (e.g., "bushing", "damper", etc.)

                otherwise
                    error('Unknown type: %s', currentConType);
            end
        end
    end
end