function preloadVecCombined = createPreloadVec(preloadFromSignalVec, numBodies, varargin)
    % Combines preload vectors from multiple bush structures and preloadFromSignalVec
    %
    % Inputs:
    %   preloadFromSignalVec - Structure containing drive properties
    %   numBodies - Number of bushings or specific parameter defining the size of preload vector
    %   varargin - Pairs of bush structures and their corresponding preload data (or empty placeholder)
    %
    % Outputs:
    %   preload - Combined preload vector
    %
    % Description:
    %   This function creates preload vectors from multiple bush structures passed through varargin, 
    %   using the number of bushings (numBodies) for each. It sums these preload vectors together.
    %   The function also considers each bush structure's active status and constructs preload vectors
    %   accordingly. It then combines these vectors with the signal vector from preloadFromSignalVec to 
    %   form the final combined preload vector

    % Initialize the cumulative preload vector for all bush structures
    preloadFromBushStruct = zeros(6 * numBodies, 1);

    % Iterate over each pair of bush structure and its corresponding preload data
    for i = 1:2:length(varargin)
        % Extract the bush structure
        BushStruct = varargin{i};

        % Check if the next element is preload data or an empty placeholder
        if i+1 <= length(varargin) && ~isempty(varargin{i+1})
            % The next element is preload data
            preloadData = varargin{i+1};
            preloadFromBushStruct = preloadFromBushStruct + createBushStructPreloadVec(BushStruct, numBodies, preloadData);
        else
            % No preload data provided, or it's an empty placeholder
            preloadFromBushStruct = preloadFromBushStruct + createBushStructPreloadVec(BushStruct, numBodies);
        end
    end

    % Combine the bush structures' preload vector with the signal vector
    preloadVecCombined = preloadFromBushStruct + preloadFromSignalVec;
end
