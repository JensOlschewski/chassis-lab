function [charStructOut] = createBushMatrices(bushStruct, charStructIn)
   
    % Initialize output variables within charStructOut for clarity and encapsulation
    [kMatrixLok, kAvgMatrixLok, cMatrixLok, cAvgMatrixLok, cMatrixLokRange, cAvgMatrixLokRange, kMatrixLokNullspace, kAvgMatrixLokNullspace, kAvgVals, charStructOut] = deal([]);
    
    % Only process if bushing is active
    if ~bushStruct.active 
        return; % Exit if bushing is not active
    end
    
    % Extract stiffness from charStruct
    [kVals, kAvgVals] = extractStiffness(charStructIn, bushStruct.loadStruct.currentDeflection);

    % Process based on topology
    switch bushStruct.topology
        case 'additional'
            [kMatrixLok, kAvgMatrixLok] = processAdditionalTopology(kVals, kAvgVals);

        case {'primaryFlex', 'secondaryFlex'}
            % Remember to finalize this section as per your application's needs
            % [charStructOut.cMatrixLok, charStructOut.kMatrixLok, charStructOut.cMatrixLokRange, charStructOut.kMatrixLokNullspace] = processFlexTopology(bushStruct);

        case {'primary', 'secondary'}
            [kMatrixLok, kAvgMatrixLok, cMatrixLok, cAvgMatrixLok, cMatrixLokRange, cAvgMatrixLokRange, kMatrixLokNullspace, kAvgMatrixLokNullspace] = processPrimarySecondaryTopology(kVals, kAvgVals, bushStruct);

        otherwise
            error('Unknown Bushing Topology Type');
            
    end

    % At the very end, assign all computed matrices to charStructOut
    charStructOut = charStructIn;
    charStructOut.kMatrixLok = kMatrixLok;
    charStructOut.kAvgMatrixLok = kAvgMatrixLok;
    charStructOut.cMatrixLok = cMatrixLok;
    charStructOut.cAvgMatrixLok = cAvgMatrixLok;
    charStructOut.cMatrixLokRange = cMatrixLokRange;
    charStructOut.cAvgMatrixLokRange = cAvgMatrixLokRange;
    charStructOut.kMatrixLokNullspace = kMatrixLokNullspace;
    charStructOut.kAvgMatrixLokNullspace = kAvgMatrixLokNullspace;
end

% Helper Functions ----------------------------------------------------------------
% Handles the 'additional' topology for bushings.
function [kMatrixLok, kAvgMatrixLok] = processAdditionalTopology(kVals, kAvgVals)
    kMatrixLok = diag(kVals);
    kAvgMatrixLok = diag(kAvgVals);
end


% Handles the 'primary' and 'secondary' topologies for bushings.
function [kMatrixLok, kAvgMatrixLok, cMatrixLok, cAvgMatrixLok, cMatrixLokRange, cAvgMatrixLokRange, kMatrixLokNullspace, kAvgMatrixLokNullspace] = processPrimarySecondaryTopology(kVals, kAvgVals, bushStruct)
    % Initialize matrices based on stiffness values
    kMatrixLok = diag(kVals);
    kAvgMatrixLok = diag(kAvgVals);
    
    % Compute local compliance matrices from stiffness matrices
    cMatrixLok = computeLocCompMat(kMatrixLok);
    cAvgMatrixLok = computeLocCompMat(kAvgMatrixLok);
    
    % Determine null space contributions
    [cMatrixLokRange, kMatrixLokNullspace] = determineNullSpaceContributions(cMatrixLok, kMatrixLok, bushStruct.charStruct.NullSpaceMatrix);
    [cAvgMatrixLokRange, kAvgMatrixLokNullspace] = determineNullSpaceContributions(cAvgMatrixLok, kAvgMatrixLok, bushStruct.charStruct.NullSpaceMatrix);
    
    % Adjust nullspace matrices if joint type is 'Symmetriebedingung'
    if strcmp(bushStruct.jointType, 'Symmetriebedingung')
        kMatrixLokNullspace = 1 ./ (cMatrixLokRange + (cMatrixLokRange == 0)) .* (cMatrixLokRange ~= 0);
        kAvgMatrixLokNullspace = 1 ./ (cAvgMatrixLokRange + (cAvgMatrixLokRange == 0)) .* (cAvgMatrixLokRange ~= 0);
    end
end